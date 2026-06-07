import 'package:flutter/material.dart';
import 'models/server_profile.dart';
import 'screens/servers_screen.dart';
import 'screens/session_shell.dart';
import 'services/api.dart';
import 'services/server_store.dart';
import 'services/push_service.dart';
import 'services/ssh_tunnel_service.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await PushService.instance.init();
  runApp(const BunnyApp());
}

class BunnyApp extends StatefulWidget {
  const BunnyApp({super.key});

  @override
  State<BunnyApp> createState() => _BunnyAppState();
}

class _BunnyAppState extends State<BunnyApp> {
  final _store = ServerStore();
  late final SshTunnelService _tunnel;
  ServerProfile? _connectedProfile;
  BunnyApi? _api;
  int _sessionEpoch = 0;

  @override
  void initState() {
    super.initState();
    _tunnel = SshTunnelService(_store);
  }

  @override
  void dispose() {
    _tunnel.dispose();
    super.dispose();
  }

  void _onConnected(ServerProfile profile, BunnyApi api) {
    setState(() {
      _connectedProfile = profile;
      _api = api;
      _sessionEpoch++;
    });
  }

  void _onDisconnected() {
    setState(() {
      _connectedProfile = null;
      _api = null;
      _sessionEpoch++;
    });
  }

  Future<void> _tryRestoreSession() async {
    final profiles = await _store.listProfiles();
    for (final p in profiles) {
      final token = await _store.getBunnyToken(p.id);
      if (token == null) continue;
      try {
        await _tunnel.connect(p);
        final ok = await _tunnel.verifyBunnyReachable(p);
        if (!ok) {
          await _tunnel.disconnect();
          continue;
        }
        final api = BunnyApi(baseUrl: p.localBunnyBaseUrl, profileId: p.id);
        await api.me();
        _onConnected(p, api);
        return;
      } catch (_) {
        await _tunnel.disconnect();
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'bunny',
      theme: ThemeData(
        brightness: Brightness.dark,
        colorScheme: ColorScheme.dark(
          primary: const Color(0xFF9498FF),
          surface: const Color(0xFF0D1117),
        ),
      ),
      home: _connectedProfile != null && _api != null
          ? SessionShell(
              key: ValueKey('session-$_sessionEpoch'),
              profile: _connectedProfile!,
              api: _api!,
              tunnel: _tunnel,
              store: _store,
              onDisconnect: _onDisconnected,
            )
          : _BootstrapScreen(
              store: _store,
              tunnel: _tunnel,
              onReady: _onConnected,
              onRestore: _tryRestoreSession,
            ),
    );
  }
}

class _BootstrapScreen extends StatefulWidget {
  const _BootstrapScreen({
    required this.store,
    required this.tunnel,
    required this.onReady,
    required this.onRestore,
  });

  final ServerStore store;
  final SshTunnelService tunnel;
  final void Function(ServerProfile profile, BunnyApi api) onReady;
  final Future<void> Function() onRestore;

  @override
  State<_BootstrapScreen> createState() => _BootstrapScreenState();
}

class _BootstrapScreenState extends State<_BootstrapScreen> {
  bool _restoring = true;

  @override
  void initState() {
    super.initState();
    _restore();
  }

  Future<void> _restore() async {
    await widget.onRestore();
    if (mounted) setState(() => _restoring = false);
  }

  @override
  Widget build(BuildContext context) {
    if (_restoring) {
      return const Scaffold(
        backgroundColor: Color(0xFF0D1117),
        body: Center(child: CircularProgressIndicator()),
      );
    }
    return ServersScreen(
      store: widget.store,
      tunnel: widget.tunnel,
      onConnected: widget.onReady,
    );
  }
}
