import 'dart:convert';
import 'dart:math';
import 'dart:typed_data';

import 'package:encrypt/encrypt.dart' as enc;
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

/// AES-256-GCM envelope for sensitive blobs (SSH password, private keys).
class EncryptedStore {
  EncryptedStore({FlutterSecureStorage? storage})
      : _storage = storage ?? const FlutterSecureStorage();

  final FlutterSecureStorage _storage;
  static const _masterKeyName = 'bunny_cred_master_v1';

  Future<enc.Key> _masterKey() async {
    var raw = await _storage.read(key: _masterKeyName);
    if (raw == null) {
      final bytes = List<int>.generate(32, (_) => Random.secure().nextInt(256));
      raw = base64Encode(bytes);
      await _storage.write(key: _masterKeyName, value: raw);
    }
    return enc.Key(base64Decode(raw));
  }

  Future<String> encryptString(String plaintext) async {
    final key = await _masterKey();
    final iv = enc.IV.fromSecureRandom(12);
    final encrypter = enc.Encrypter(enc.AES(key, mode: enc.AESMode.gcm));
    final encrypted = encrypter.encrypt(plaintext, iv: iv);
    return base64Encode(
      Uint8List.fromList([1, ...iv.bytes, ...encrypted.bytes]),
    );
  }

  Future<String> decryptString(String envelope) async {
    final key = await _masterKey();
    final bytes = base64Decode(envelope);
    if (bytes.isEmpty || bytes.first != 1) {
      throw StateError('Unsupported credential envelope');
    }
    final iv = enc.IV(bytes.sublist(1, 13));
    final cipher = bytes.sublist(13);
    final encrypter = enc.Encrypter(enc.AES(key, mode: enc.AESMode.gcm));
    return encrypter.decrypt(enc.Encrypted(cipher), iv: iv);
  }
}
