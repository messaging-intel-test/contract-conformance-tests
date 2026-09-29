import 'package:flutter_test/flutter_test.dart';
import 'package:ores_common_mobile_host/ores_common_mobile_host.dart';

void main() {
  test('mobile host config accepts one valid loopback origin port', () {
    final config = MobileHostConfig(
      productId: 'scintilla',
      deviceId: 'device-1',
      originPort: 18080,
      mode: MobileHostingMode.persistentOrigin,
      relayUrl: Uri.parse('https://relay.example.com/device'),
    );

    expect(config.validate, returnsNormally);
  });

  test('invalid origin port is rejected', () {
    final config = MobileHostConfig(
      productId: 'beamscale',
      deviceId: 'device-2',
      originPort: 70000,
      mode: MobileHostingMode.wakeAndDrain,
      relayUrl: Uri.parse('https://relay.example.com/device'),
    );

    expect(config.validate, throwsArgumentError);
  });

  test('relay transport must use credential-free https', () {
    final insecure = MobileHostConfig(
      productId: 'scintilla',
      deviceId: 'device-1',
      originPort: 18080,
      mode: MobileHostingMode.persistentOrigin,
      relayUrl: Uri.parse('http://relay.example.com/device'),
    );
    final embeddedCredentials = MobileHostConfig(
      productId: 'scintilla',
      deviceId: 'device-1',
      originPort: 18080,
      mode: MobileHostingMode.persistentOrigin,
      relayUrl: Uri.parse('https://user:secret@relay.example.com/device'),
    );

    expect(insecure.validate, throwsArgumentError);
    expect(embeddedCredentials.validate, throwsArgumentError);
  });

  test('platform capabilities remain explicit', () {
    const capabilities = MobileHostCapabilities(
      platform: 'ios',
      supportsPersistentOrigin: false,
      supportsWakeAndDrain: true,
      supportsBackgroundPush: true,
      minimumRepairWakeMinutes: null,
      backgroundTaskRegistered: false,
      notificationsEnabled: true,
    );

    expect(capabilities.supportsPersistentOrigin, isFalse);
    expect(capabilities.supportsWakeAndDrain, isTrue);
    expect(capabilities.backgroundTaskRegistered, isFalse);
    expect(capabilities.notificationsEnabled, isTrue);
  });
}
