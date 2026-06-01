// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

import 'package:flutter/material.dart';

/// Supported local mobile wallets (Phantom-compatible encrypted deep links).
enum NativeWalletId { phantom, solflare, backpack }

/// Per-wallet deep link configuration.
class NativeWalletConfig {
  final NativeWalletId id;
  final String displayName;
  final String connectUrl;
  final String signAndSendUrl;
  final String signOnlyUrl;
  /// Redirect path segment, e.g. `phantom/connect` → ignitepay://phantom/connect
  final String connectRedirectPath;
  final String signRedirectPath;
  final String signOnlyRedirectPath;
  /// Query param for wallet encryption pubkey on connect callback.
  final String encryptionPublicKeyParam;
  final String prefsPrefix;
  final Color accentColor;

  const NativeWalletConfig({
    required this.id,
    required this.displayName,
    required this.connectUrl,
    required this.signAndSendUrl,
    required this.signOnlyUrl,
    required this.connectRedirectPath,
    required this.signRedirectPath,
    required this.signOnlyRedirectPath,
    required this.encryptionPublicKeyParam,
    required this.prefsPrefix,
    required this.accentColor,
  });
}

class NativeWalletConfigs {
  NativeWalletConfigs._();

  static const phantom = NativeWalletConfig(
    id: NativeWalletId.phantom,
    displayName: 'Phantom',
    // Use native app scheme to ensure direct app-to-app deep link.
    connectUrl: 'phantom://v1/connect',
    signAndSendUrl: 'phantom://v1/signAndSendTransaction',
    signOnlyUrl: 'phantom://v1/signTransaction',
    connectRedirectPath: 'phantom/connect',
    signRedirectPath: 'phantom/sign',
    signOnlyRedirectPath: 'phantom/signonly',
    encryptionPublicKeyParam: 'phantom_encryption_public_key',
    prefsPrefix: 'phantom_',
    accentColor: Color(0xFFAB9FF2),
  );

  static const solflare = NativeWalletConfig(
    id: NativeWalletId.solflare,
    displayName: 'Solflare',
    connectUrl: 'solflare://v1/connect',
    signAndSendUrl: 'solflare://v1/signAndSendTransaction',
    signOnlyUrl: 'solflare://v1/signTransaction',
    connectRedirectPath: 'solflare/connect',
    signRedirectPath: 'solflare/sign',
    signOnlyRedirectPath: 'solflare/signonly',
    encryptionPublicKeyParam: 'solflare_encryption_public_key',
    prefsPrefix: 'solflare_',
    accentColor: Color(0xFFFC8C03),
  );

  static const backpack = NativeWalletConfig(
    id: NativeWalletId.backpack,
    displayName: 'Backpack',
    connectUrl: 'https://backpack.app/ul/v1/connect',
    signAndSendUrl: 'https://backpack.app/ul/v1/signAndSendTransaction',
    signOnlyUrl: 'https://backpack.app/ul/v1/signTransaction',
    connectRedirectPath: 'backpack/connect',
    signRedirectPath: 'backpack/sign',
    signOnlyRedirectPath: 'backpack/signonly',
    encryptionPublicKeyParam: 'wallet_encryption_public_key',
    prefsPrefix: 'backpack_',
    accentColor: Color(0xFFE33E3F),
  );

  static const List<NativeWalletConfig> mobileWallets = [
    phantom,
    solflare,
    backpack,
  ];

  static NativeWalletConfig byId(NativeWalletId id) {
    return mobileWallets.firstWhere((c) => c.id == id);
  }
}
