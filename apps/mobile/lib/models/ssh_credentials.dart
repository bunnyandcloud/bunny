class SshCredentials {
  SshCredentials.password(this.password)
      : privateKeyPem = null,
        keyPassphrase = null;

  SshCredentials.privateKey({
    required this.privateKeyPem,
    this.keyPassphrase,
  }) : password = null;

  final String? password;
  final String? privateKeyPem;
  final String? keyPassphrase;

  bool get usesPrivateKey => privateKeyPem != null && privateKeyPem!.isNotEmpty;
}
