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
  final _mfaCode = TextEditingController();
  bool _loading = false;
  String? _error;
  String? _mfaChallengeToken;
  bool _useRecovery = false;

  Future<void> _submitPassword() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final api = BunnyApi(
        baseUrl: widget.profile.localBunnyBaseUrl,
        profileId: widget.profile.id,
      );
      final result = await api.login(_email.text.trim(), _password.text);
      if (result.mfaRequired) {
        setState(() {
          _mfaChallengeToken = result.mfaChallengeToken;
          _loading = false;
        });
        return;
      }
      if (mounted) {
        Navigator.pop(context, BunnyLoginResult(profile: widget.profile));
      }
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _submitMfa() async {
    final token = _mfaChallengeToken;
    if (token == null) return;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final api = BunnyApi(
        baseUrl: widget.profile.localBunnyBaseUrl,
        profileId: widget.profile.id,
      );
      await api.verifyMfa(_mfaCode.text.trim(), token);
      if (mounted) {
        Navigator.pop(context, BunnyLoginResult(profile: widget.profile));
      }
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final mfaStep = _mfaChallengeToken != null;
    return Scaffold(
      backgroundColor: const Color(0xFF0D1117),
      appBar: AppBar(
        title: Text(mfaStep ? 'Two-factor auth' : 'bunny login'),
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
              Text(
                mfaStep ? 'Enter your authenticator code' : 'Sign in to your agent',
                style: const TextStyle(
                  fontSize: 22,
                  fontWeight: FontWeight.bold,
                  color: Color(0xFF9498FF),
                ),
              ),
              const SizedBox(height: 8),
              if (!mfaStep)
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
              if (!mfaStep) ...[
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
              ] else ...[
                TextField(
                  controller: _mfaCode,
                  decoration: InputDecoration(
                    labelText: _useRecovery ? 'Recovery code' : '6-digit code',
                    filled: true,
                  ),
                  keyboardType: _useRecovery ? TextInputType.text : TextInputType.number,
                ),
                TextButton(
                  onPressed: () => setState(() {
                    _useRecovery = !_useRecovery;
                    _mfaCode.clear();
                  }),
                  child: Text(
                    _useRecovery
                        ? 'Use authenticator code instead'
                        : 'Use a recovery code',
                  ),
                ),
              ],
              const SizedBox(height: 24),
              FilledButton(
                onPressed: _loading
                    ? null
                    : (mfaStep ? _submitMfa : _submitPassword),
                child: Text(_loading
                    ? 'Please wait…'
                    : (mfaStep ? 'Verify' : 'Sign in')),
              ),
              if (mfaStep)
                TextButton(
                  onPressed: _loading
                      ? null
                      : () => setState(() {
                            _mfaChallengeToken = null;
                            _mfaCode.clear();
                            _useRecovery = false;
                          }),
                  child: const Text('Back to sign in'),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
