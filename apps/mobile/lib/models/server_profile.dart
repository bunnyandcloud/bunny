class ServerProfile {
  ServerProfile({
    required this.id,
    required this.name,
    required this.host,
    this.sshPort = 22,
    required this.sshUsername,
    this.bunnyPort = 7681,
    this.localForwardPort = 17681,
    this.authType = ServerAuthType.password,
  });

  final String id;
  final String name;
  final String host;
  final int sshPort;
  final String sshUsername;
  /// Port where bunny agent listens on the remote host (usually 127.0.0.1:bunnyPort).
  final int bunnyPort;
  /// Local port on the phone used for SSH -L forward.
  final int localForwardPort;
  final ServerAuthType authType;

  String get localBunnyBaseUrl => 'http://127.0.0.1:$localForwardPort';

  Map<String, dynamic> toJson() => {
        'id': id,
        'name': name,
        'host': host,
        'sshPort': sshPort,
        'sshUsername': sshUsername,
        'bunnyPort': bunnyPort,
        'localForwardPort': localForwardPort,
        'authType': authType.name,
      };

  factory ServerProfile.fromJson(Map<String, dynamic> json) {
    return ServerProfile(
      id: json['id'] as String,
      name: json['name'] as String,
      host: json['host'] as String,
      sshPort: json['sshPort'] as int? ?? 22,
      sshUsername: json['sshUsername'] as String,
      bunnyPort: json['bunnyPort'] as int? ?? 7681,
      localForwardPort: json['localForwardPort'] as int? ?? 17681,
      authType: ServerAuthType.values.byName(json['authType'] as String? ?? 'password'),
    );
  }
}

enum ServerAuthType {
  password,
  privateKey,
}
