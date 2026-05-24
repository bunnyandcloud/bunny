import 'package:flutter/material.dart';
import 'package:speech_to_text/speech_to_text.dart' as stt;

class VoiceSheet extends StatefulWidget {
  const VoiceSheet({
    super.key,
    required this.onProposed,
  });

  final void Function(String transcript) onProposed;

  @override
  State<VoiceSheet> createState() => _VoiceSheetState();
}

class _VoiceSheetState extends State<VoiceSheet> {
  final stt.SpeechToText _speech = stt.SpeechToText();
  bool _listening = false;
  String _text = '';

  Future<void> _toggleListen() async {
    if (!_listening) {
      final ok = await _speech.initialize();
      if (!ok) return;
      setState(() => _listening = true);
      _speech.listen(
        onResult: (r) => setState(() => _text = r.recognizedWords),
      );
    } else {
      await _speech.stop();
      setState(() => _listening = false);
      widget.onProposed(_text);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(_listening ? 'Listening…' : 'Push to talk', style: const TextStyle(fontWeight: FontWeight.bold)),
          const SizedBox(height: 12),
          Text(_text.isEmpty ? 'Say a command…' : _text),
          const SizedBox(height: 16),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceEvenly,
            children: [
              IconButton.filled(
                icon: Icon(_listening ? Icons.stop : Icons.mic),
                onPressed: _toggleListen,
              ),
              TextButton(onPressed: () => Navigator.pop(context, 'insert'), child: const Text('Insert')),
              TextButton(onPressed: () => Navigator.pop(context, 'run'), child: const Text('Run')),
              TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
            ],
          ),
          const Text('Dangerous commands require confirmation', style: TextStyle(fontSize: 11, color: Colors.grey)),
        ],
      ),
    );
  }
}
