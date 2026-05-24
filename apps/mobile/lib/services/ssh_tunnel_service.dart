import 'dart:async';
import 'dart:io';

import 'package:dartssh2/dartssh2.dart';
import '../models/server_profile.dart';
import 'server_store.dart';

enum TunnelStatus { disconnected, connecting, connected, error }

class SshTunnelState {
  SshTunnelState({
    required this.status,
    this.message,
    this.profile,
  });

  final TunnelStatus status;
  final String? message;
  final ServerProfile? profile;
}

/// SSH local forward: phone 127.0.0.1:localPort → remote 127.0.0.1:bunnyPort
class SshTunnelService {
  SshTunnelService(this._store);

  final ServerStore _store;
  SSHClient? _client;
  ServerSocket? _listener;
  final _stateController = StreamController<SshTunnelState>.broadcast();
  StreamSubscription<Socket>? _acceptSub;

  Stream<SshTunnelState> get states => _stateController.stream;
  ServerProfile? _activeProfile;

  ServerProfile? get activeProfile => _activeProfile;
  bool get isConnected => _client != null && _listener != null;

  Future<void> connect(ServerProfile profile) async {
    await disconnect();
    _emit(TunnelStatus.connecting, profile: profile, message: 'Connecting SSH…');

    try {
      await _store.migrateLegacyPassword(profile.id);
      final creds = await _store.getSshCredentials(profile.id);
      if (creds == null) {
        throw Exception('SSH credentials not saved for this server');
      }

      final socket = await SSHSocket.connect(
        profile.host,
        profile.sshPort,
        timeout: const Duration(seconds: 20),
      );

      if (profile.authType == ServerAuthType.privateKey || creds.usesPrivateKey) {
        final pem = creds.privateKeyPem;
        if (pem == null || pem.isEmpty) {
          throw Exception('Private key missing for this server');
        }
        final keys = SSHKeyPair.fromPem(
          pem,
          creds.keyPassphrase?.isNotEmpty == true ? creds.keyPassphrase : null,
        );
        _client = SSHClient(
          socket,
          username: profile.sshUsername,
          identities: keys,
        );
      } else {
        final password = creds.password;
        if (password == null || password.isEmpty) {
          throw Exception('SSH password not saved for this server');
        }
        _client = SSHClient(
          socket,
          username: profile.sshUsername,
          onPasswordRequest: () => password,
        );
      }

      await _client!.authenticated;

      _listener = await ServerSocket.bind(
        InternetAddress.loopbackIPv4,
        profile.localForwardPort,
        shared: true,
      );

      final client = _client!;
      final remotePort = profile.bunnyPort;
      _acceptSub = _listener!.listen((local) async {
        try {
          final forward = await client.forwardLocal('127.0.0.1', remotePort);
          unawaited(forward.stream.cast<List<int>>().pipe(local));
          unawaited(local.cast<List<int>>().pipe(forward.sink));
        } catch (e) {
          try {
            await local.close();
          } catch (_) {}
        }
      });

      _activeProfile = profile;
      _emit(TunnelStatus.connected, profile: profile, message: 'Tunnel active');
    } catch (e) {
      await disconnect();
      _emit(TunnelStatus.error, profile: profile, message: e.toString());
      rethrow;
    }
  }

  Future<bool> verifyBunnyReachable(ServerProfile profile) async {
    try {
      final client = HttpClient();
      final request = await client
          .getUrl(Uri.parse('${profile.localBunnyBaseUrl}/api/v1/agent/info'))
          .timeout(const Duration(seconds: 5));
      final response = await request.close();
      final ok = response.statusCode == 200;
      client.close(force: true);
      return ok;
    } catch (_) {
      return false;
    }
  }

  Future<void> disconnect() async {
    await _acceptSub?.cancel();
    _acceptSub = null;

    final listener = _listener;
    _listener = null;
    if (listener != null) {
      try {
        await listener.close();
      } catch (_) {}
    }

    final client = _client;
    _client = null;
    _activeProfile = null;
    if (client != null) {
      try {
        client.close();
      } catch (_) {}
    }
    _emit(TunnelStatus.disconnected, message: 'Disconnected');
  }

  void dispose() {
    disconnect();
    _stateController.close();
  }

  void _emit(
    TunnelStatus status, {
    ServerProfile? profile,
    String? message,
  }) {
    if (!_stateController.isClosed) {
      _stateController.add(
        SshTunnelState(status: status, profile: profile, message: message),
      );
    }
  }
}
