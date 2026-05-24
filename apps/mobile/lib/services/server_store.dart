import 'dart:convert';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import '../models/server_profile.dart';
import '../models/ssh_credentials.dart';
import 'encrypted_store.dart';

class ServerStore {
  ServerStore({
    FlutterSecureStorage? storage,
    EncryptedStore? encrypted,
  })  : _storage = storage ?? const FlutterSecureStorage(),
        _encrypted = encrypted ?? EncryptedStore(storage: storage);

  final FlutterSecureStorage _storage;
  final EncryptedStore _encrypted;
  static const _profilesKey = 'bunny_server_profiles';

  Future<List<ServerProfile>> listProfiles() async {
    final raw = await _storage.read(key: _profilesKey);
    if (raw == null || raw.isEmpty) return [];
    final list = jsonDecode(raw) as List<dynamic>;
    return list
        .map((e) => ServerProfile.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> saveProfile(ServerProfile profile) async {
    final profiles = await listProfiles();
    final idx = profiles.indexWhere((p) => p.id == profile.id);
    if (idx >= 0) {
      profiles[idx] = profile;
    } else {
      profiles.add(profile);
    }
    await _storage.write(
      key: _profilesKey,
      value: jsonEncode(profiles.map((p) => p.toJson()).toList()),
    );
  }

  Future<void> deleteProfile(String id) async {
    final profiles = await listProfiles();
    profiles.removeWhere((p) => p.id == id);
    await _storage.write(
      key: _profilesKey,
      value: jsonEncode(profiles.map((p) => p.toJson()).toList()),
    );
    await _storage.delete(key: _credKey(id));
    await _storage.delete(key: 'bunny_token_$id');
  }

  Future<void> saveSshCredentials(String profileId, SshCredentials creds) async {
    final payload = <String, String>{};
    if (creds.password != null) {
      payload['password'] = creds.password!;
    }
    if (creds.privateKeyPem != null) {
      payload['privateKeyPem'] = creds.privateKeyPem!;
    }
    if (creds.keyPassphrase != null && creds.keyPassphrase!.isNotEmpty) {
      payload['keyPassphrase'] = creds.keyPassphrase!;
    }
    final envelope = await _encrypted.encryptString(jsonEncode(payload));
    await _storage.write(key: _credKey(profileId), value: envelope);
  }

  Future<SshCredentials?> getSshCredentials(String profileId) async {
    final envelope = await _storage.read(key: _credKey(profileId));
    if (envelope == null || envelope.isEmpty) return null;
    final plain = await _encrypted.decryptString(envelope);
    final map = jsonDecode(plain) as Map<String, dynamic>;
    if (map.containsKey('privateKeyPem')) {
      return SshCredentials.privateKey(
        privateKeyPem: map['privateKeyPem'] as String,
        keyPassphrase: map['keyPassphrase'] as String?,
      );
    }
    return SshCredentials.password(map['password'] as String? ?? '');
  }

  /// Legacy plain password migration path.
  Future<void> migrateLegacyPassword(String profileId) async {
    final legacy = await _storage.read(key: 'bunny_ssh_password_$profileId');
    if (legacy == null) return;
    await saveSshCredentials(profileId, SshCredentials.password(legacy));
    await _storage.delete(key: 'bunny_ssh_password_$profileId');
  }

  Future<void> saveBunnyToken(String profileId, String token) async {
    await _storage.write(key: 'bunny_token_$profileId', value: token);
  }

  Future<String?> getBunnyToken(String profileId) =>
      _storage.read(key: 'bunny_token_$profileId');

  Future<void> clearBunnyToken(String profileId) =>
      _storage.delete(key: 'bunny_token_$profileId');

  String _credKey(String profileId) => 'bunny_ssh_cred_$profileId';

  Future<String?> readPlain(String key) => _storage.read(key: key);

  Future<void> writePlain(String key, String value) =>
      _storage.write(key: key, value: value);
}
