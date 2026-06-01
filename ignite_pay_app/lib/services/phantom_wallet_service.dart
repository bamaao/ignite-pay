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

import 'package:ignite_pay_app/services/native_deep_link_wallet_service.dart';
import 'package:ignite_pay_app/services/native_wallet_config.dart';
import 'package:ignite_pay_app/services/wallet_service.dart';

/// Backward-compatible alias for Phantom native deep link wallet.
class PhantomWalletService extends NativeDeepLinkWalletService {
  static final PhantomWalletService _instance = PhantomWalletService._();
  factory PhantomWalletService() => _instance;
  PhantomWalletService._() : super.withConfig(NativeWalletConfigs.phantom);
}

/// Solflare native deep link wallet.
class SolflareWalletService extends NativeDeepLinkWalletService {
  static final SolflareWalletService _instance = SolflareWalletService._();
  factory SolflareWalletService() => _instance;
  SolflareWalletService._() : super.withConfig(NativeWalletConfigs.solflare);
}

/// Backpack native deep link wallet.
class BackpackWalletService extends NativeDeepLinkWalletService {
  static final BackpackWalletService _instance = BackpackWalletService._();
  factory BackpackWalletService() => _instance;
  BackpackWalletService._() : super.withConfig(NativeWalletConfigs.backpack);
}

WalletService nativeWalletService(NativeWalletId id) =>
    NativeDeepLinkWalletService.forWallet(id);
