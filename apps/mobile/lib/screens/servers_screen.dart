import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:uuid/uuid.dart';
import '../models/server_profile.dart';
import '../models/ssh_credentials.dart';
import '../services/api.dart';
import '../services/server_store.dart';
import '../services/ssh_tunnel_service.dart';
import 'connect_server_screen.dart';
import 'login_screen.dart';

class ServersScreen extends StatefulWidget {
  const ServersScreen({
    super.key,
    required this.store,
    required this.tunnel,
    required this.onConnected,
  });

  final ServerStore store;
  final SshTunnelService tunnel;
  final void Function(ServerProfile profile, BunnyApi api) onConnected;

  @override
  State<ServersScreen> createState() => _ServersScreenState();
}

class _ServersScreenState extends State<ServersScreen> {
  List<ServerProfile> _profiles = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final list = await widget.store.listProfiles();
    setState(() {
      _profiles = list;
      _loading = false;
    });
  }

  Future<void> _addServer() async {
    final result = await Navigator.of(context).push<ServerProfile>(
      MaterialPageRoute(
        builder: (_) => AddServerScreen(store: widget.store),
      ),
    );
    if (result != null) await _load();
  }

  Future<void> _openServer(ServerProfile profile) async {
    final tunnelOk = await Navigator.of(context).push<bool>(
      MaterialPageRoute(
        builder: (_) => ConnectServerScreen(
          profile: profile,
          store: widget.store,
          tunnel: widget.tunnel,
        ),
      ),
    );
    if (tunnelOk != true || !mounted) return;

    final loginResult = await Navigator.of(context).push<BunnyLoginResult>(
      MaterialPageRoute(
        builder: (_) => BunnyLoginScreen(profile: profile, store: widget.store),
      ),
    );
    if (loginResult == null || !mounted) return;

    final api = BunnyApi(
      baseUrl: profile.localBunnyBaseUrl,
      profileId: profile.id,
    );
    widget.onConnected(profile, api);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFF0D1117),
      appBar: AppBar(
        title: const Text('bunny'),
        backgroundColor: const Color(0xFF161B22),
        actions: [
          IconButton(icon: const Icon(Icons.add), onPressed: _addServer),
        ],
      ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : _profiles.isEmpty
              ? _emptyState()
              : ListView.builder(
                  itemCount: _profiles.length,
                  itemBuilder: (_, i) {
                    final p = _profiles[i];
                    final active = widget.tunnel.activeProfile?.id == p.id &&
                        widget.tunnel.isConnected;
                    return ListTile(
                      title: Text(p.name, style: const TextStyle(color: Colors.white)),
                      subtitle: Text(
                        '${p.sshUsername}@${p.host}:${p.sshPort} · ${p.authType.name}',
                        style: const TextStyle(color: Color(0xFF8B949E), fontSize: 12),
                      ),
                      trailing: active
                          ? const Icon(Icons.link, color: Color(0xFF3FB950))
                          : const Icon(Icons.chevron_right),
                      onTap: () => _openServer(p),
                      onLongPress: () => _confirmDelete(p),
                    );
                  },
                ),
    );
  }

  Widget _emptyState() {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(Icons.dns_outlined, size: 48, color: Color(0xFF8B949E)),
            const SizedBox(height: 16),
            const Text(
              'Connect to your server',
              style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
            ),
            const SizedBox(height: 8),
            const Text(
              'Add host + SSH credentials. bunny connects via an integrated SSH tunnel — store-ready, no open ports on the server.',
              textAlign: TextAlign.center,
              style: TextStyle(color: Color(0xFF8B949E)),
            ),
            const SizedBox(height: 24),
            FilledButton.icon(
              onPressed: _addServer,
              icon: const Icon(Icons.add),
              label: const Text('Add server'),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _confirmDelete(ServerProfile p) async {
    final yes = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete server?'),
        content: Text('Remove ${p.name}'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          TextButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Delete')),
        ],
      ),
    );
    if (yes == true) {
      await widget.store.deleteProfile(p.id);
      if (widget.tunnel.activeProfile?.id == p.id) {
        await widget.tunnel.disconnect();
      }
      await _load();
    }
  }
}

class BunnyLoginResult {
  BunnyLoginResult({required this.profile});
  final ServerProfile profile;
}

class AddServerScreen extends StatefulWidget {
  const AddServerScreen({super.key, required this.store});
  final ServerStore store;

  @override
  State<AddServerScreen> createState() => _AddServerScreenState();
}

class _AddServerScreenState extends State<AddServerScreen> {
  final _name = TextEditingController();
  final _host = TextEditingController();
  final _user = TextEditingController(text: 'root');
  final _sshPort = TextEditingController(text: '22');
  final _bunnyPort = TextEditingController(text: '7681');
  final _localPort = TextEditingController(text: '17681');
  final _password = TextEditingController();
  final _privateKey = TextEditingController();
  final _keyPassphrase = TextEditingController();
  ServerAuthType _authType = ServerAuthType.password;
  bool _saving = false;
  String? _error;

  Future<void> _pickKeyFile() async {
    final result = await FilePicker.platform.pickFiles(
      type: FileType.custom,
      allowedExtensions: ['pem', 'key'],
      withData: true,
    );
    if (result == null || result.files.isEmpty) return;
    final file = result.files.first;
    final text = file.bytes != null
        ? String.fromCharCodes(file.bytes!)
        : null;
    if (text != null) {
      setState(() => _privateKey.text = text);
    }
  }

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      final profile = ServerProfile(
        id: const Uuid().v4(),
        name: _name.text.trim().isEmpty ? _host.text.trim() : _name.text.trim(),
        host: _host.text.trim(),
        sshPort: int.tryParse(_sshPort.text) ?? 22,
        sshUsername: _user.text.trim(),
        bunnyPort: int.tryParse(_bunnyPort.text) ?? 7681,
        localForwardPort: int.tryParse(_localPort.text) ?? 17681,
        authType: _authType,
      );

      final SshCredentials creds;
      if (_authType == ServerAuthType.privateKey) {
        final pem = _privateKey.text.trim();
        if (pem.isEmpty) {
          throw Exception('Paste or import a private key (PEM)');
        }
        creds = SshCredentials.privateKey(
          privateKeyPem: pem,
          keyPassphrase: _keyPassphrase.text.trim().isEmpty
              ? null
              : _keyPassphrase.text,
        );
      } else {
        if (_password.text.isEmpty) {
          throw Exception('SSH password is required');
        }
        creds = SshCredentials.password(_password.text);
      }

      await widget.store.saveProfile(profile);
      await widget.store.saveSshCredentials(profile.id, creds);
      if (mounted) Navigator.pop(context, profile);
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFF0D1117),
      appBar: AppBar(title: const Text('Add server'), backgroundColor: const Color(0xFF161B22)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          _field(_name, 'Display name', 'My VPS'),
          _field(_host, 'Host', '203.0.113.10'),
          _field(_user, 'SSH username', 'ubuntu'),
          _field(_sshPort, 'SSH port', '22', keyboard: TextInputType.number),
          _field(_bunnyPort, 'bunny port on server', '7681', keyboard: TextInputType.number),
          _field(_localPort, 'Local forward port', '17681', keyboard: TextInputType.number),
          const SizedBox(height: 8),
          const Text('SSH authentication', style: TextStyle(fontWeight: FontWeight.w600)),
          const SizedBox(height: 8),
          SegmentedButton<ServerAuthType>(
            segments: const [
              ButtonSegment(
                value: ServerAuthType.password,
                label: Text('Password'),
                icon: Icon(Icons.password),
              ),
              ButtonSegment(
                value: ServerAuthType.privateKey,
                label: Text('Private key'),
                icon: Icon(Icons.vpn_key),
              ),
            ],
            selected: {_authType},
            onSelectionChanged: (s) => setState(() => _authType = s.first),
          ),
          const SizedBox(height: 12),
          if (_authType == ServerAuthType.password)
            _field(_password, 'SSH password', '••••••••', obscure: true)
          else ...[
            TextField(
              controller: _privateKey,
              maxLines: 6,
              decoration: const InputDecoration(
                labelText: 'Private key (PEM)',
                hintText: '-----BEGIN OPENSSH PRIVATE KEY-----',
                filled: true,
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 8),
            OutlinedButton.icon(
              onPressed: _pickKeyFile,
              icon: const Icon(Icons.upload_file),
              label: const Text('Import key file'),
            ),
            const SizedBox(height: 12),
            _field(_keyPassphrase, 'Key passphrase (if encrypted)', 'optional', obscure: true),
          ],
          if (_error != null) ...[
            const SizedBox(height: 8),
            Text(_error!, style: const TextStyle(color: Colors.redAccent, fontSize: 12)),
          ],
          const SizedBox(height: 8),
          const Text(
            'bunny must listen on 127.0.0.1 on the server. Run: bunny start --host 127.0.0.1',
            style: TextStyle(color: Color(0xFF8B949E), fontSize: 12),
          ),
          const SizedBox(height: 24),
          FilledButton(
            onPressed: _saving ? null : _save,
            child: Text(_saving ? 'Saving…' : 'Save server'),
          ),
        ],
      ),
    );
  }

  Widget _field(
    TextEditingController c,
    String label,
    String hint, {
    bool obscure = false,
    TextInputType? keyboard,
  }) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: TextField(
        controller: c,
        obscureText: obscure,
        keyboardType: keyboard,
        decoration: InputDecoration(
          labelText: label,
          hintText: hint,
          filled: true,
          border: const OutlineInputBorder(),
        ),
      ),
    );
  }
}
