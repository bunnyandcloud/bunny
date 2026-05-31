import 'dart:convert';
import 'dart:io' show Platform;
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:web_socket_channel/io.dart';
import 'package:web_socket_channel/web_socket_channel.dart';
import 'package:xterm/xterm.dart';
import '../services/api.dart';

class TerminalScreen extends StatefulWidget {
  const TerminalScreen({super.key, required this.api, required this.terminalId});

  final BunnyApi api;
  final String terminalId;

  @override
  State<TerminalScreen> createState() => _TerminalScreenState();
}

class _TerminalScreenState extends State<TerminalScreen> {
  late final Terminal _terminal;
  late final TerminalController _controller;
  WebSocketChannel? _channel;
  int _offset = 0;
  int _liveFence = 0;
  bool _replayDone = false;

  @override
  void initState() {
    super.initState();
    _terminal = Terminal(
      onOutput: (data) {
        _channel?.sink.add(jsonEncode({'type': 'input', 'data': data}));
      },
    );
    _controller = TerminalController();
    _connect();
  }

  Future<void> _connect() async {
    final token = await widget.api.getToken();
    final uri = Uri.parse(widget.api.terminalWsUrl(widget.terminalId, fromOffset: _offset));
    final headers = <String, dynamic>{
      if (token != null) 'Authorization': 'Bearer $token',
    };

    if (!kIsWeb && (Platform.isAndroid || Platform.isIOS || Platform.isMacOS || Platform.isLinux)) {
      _channel = IOWebSocketChannel.connect(uri, headers: headers);
    } else {
      _channel = WebSocketChannel.connect(uri);
    }

    _channel!.stream.listen((data) {
      try {
        final msg = jsonDecode(data as String) as Map<String, dynamic>;
        if (msg['type'] == 'replay') {
          final mode = msg['replay_mode'] as String? ??
              (msg['has_history'] == true ? 'recovery' : 'none');
          final snapshot = msg['snapshot_offset'] as int? ?? 0;
          if (mode == 'recovery') {
            _terminal.clear();
          }
          for (final c in (msg['chunks'] as List)) {
            _terminal.write(c['data'] as String);
            _offset = c['offset'] as int? ?? _offset;
          }
          _liveFence = snapshot;
          _replayDone = true;
        } else if (msg['type'] == 'output') {
          if (!_replayDone) return;
          final offset = msg['offset'] as int? ?? 0;
          if (offset <= _liveFence) return;
          _terminal.write(msg['data'] as String);
          _offset = offset;
        }
      } catch (_) {}
    });
    _channel!.sink.add(jsonEncode({'type': 'subscribe', 'from_offset': _offset}));
  }

  @override
  void dispose() {
    _channel?.sink.close();
    _controller.dispose();
    super.dispose();
  }

  void _sendKey(String key) {
    _channel?.sink.add(jsonEncode({'type': 'input', 'data': key}));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFF0D1117),
      appBar: AppBar(title: const Text('Terminal'), backgroundColor: const Color(0xFF161B22)),
      body: Column(
        children: [
          Expanded(child: TerminalView(_terminal, controller: _controller)),
          _TerminalKeyBar(onKey: _sendKey),
        ],
      ),
    );
  }
}

class _TerminalKeyBar extends StatelessWidget {
  const _TerminalKeyBar({required this.onKey});
  final void Function(String) onKey;

  @override
  Widget build(BuildContext context) {
    const keys = ['Esc', 'Tab', '/', ':', '|', '←', '→', '↑', '↓'];
    return Container(
      color: const Color(0xFF161B22),
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: keys.map((k) {
            return Padding(
              padding: const EdgeInsets.symmetric(horizontal: 4),
              child: ActionChip(
                label: Text(k, style: const TextStyle(fontSize: 12)),
                onPressed: () {
                  switch (k) {
                    case 'Esc':
                      onKey('\x1b');
                    case 'Tab':
                      onKey('\t');
                    case '/':
                      onKey('/');
                    case ':':
                      onKey(':');
                    case '|':
                      onKey('|');
                    case '←':
                      onKey('\x1b[D');
                    case '→':
                      onKey('\x1b[C');
                    case '↑':
                      onKey('\x1b[A');
                    case '↓':
                      onKey('\x1b[B');
                    default:
                      onKey(k);
                  }
                },
              ),
            );
          }).toList(),
        ),
      ),
    );
  }
}
