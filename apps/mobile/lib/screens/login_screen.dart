import 'package:flutter/material.dart';
import '../models/server_profile.dart';
import '../services/api.dart';
import '../services/server_store.dart';
import 'servers_screen.dart';

class BunnyLoginScreen extends StatefulWidget {
  const BunnyLoginScreen({
    super.key,
    required this.profile,
    required this.store,
  });

  final ServerProfile profile;
  final ServerStore store;

  @override
  State<BunnyLoginScreen> createState() => _BunnyLoginScreenState();
}

class _BunnyLoginScreenState extends State<BunnyLoginScreen> {
  final _email = TextEditingController();
  final _password = TextEditingController();
  bool _loading = false;
  String? _error;

  Future<void> _submit() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final api = BunnyApi(
        baseUrl: widget.profile.localBunnyBaseUrl,
        profileId: widget.profile.id,
      );
      await api.login(_email.text.trim(), _password.text);
      if (mounted) {
        Navigator.pop(context, BunnyLoginResult(profile: widget.profile));
      }
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFF0D1117),
      appBar: AppBar(
        title: const Text('bunny login'),
        backgroundColor: const Color(0xFF161B22),
      ),
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                widget.profile.name,
                style: const TextStyle(fontSize: 14, color: Color(0xFF8B949E)),
              ),
              const Text(
                'Sign in to your agent',
                style: TextStyle(fontSize: 22, fontWeight: FontWeight.bold, color: Color(0xFF58A6FF)),
              ),
              const SizedBox(height: 8),
              const Text(
                'Credentials are for bunny on your server — not your SSH password.',
                style: TextStyle(color: Color(0xFF8B949E), fontSize: 12),
              ),
              const SizedBox(height: 24),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(bottom: 12),
                  child: Text(_error!, style: const TextStyle(color: Colors.redAccent)),
                ),
              TextField(
                controller: _email,
                decoration: const InputDecoration(labelText: 'Email', filled: true),
                keyboardType: TextInputType.emailAddress,
                autocorrect: false,
              ),
              const SizedBox(height: 12),
              TextField(
                controller: _password,
                decoration: const InputDecoration(labelText: 'Password', filled: true),
                obscureText: true,
              ),
              const SizedBox(height: 24),
              FilledButton(
                onPressed: _loading ? null : _submit,
                child: Text(_loading ? 'Signing in…' : 'Sign in'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
