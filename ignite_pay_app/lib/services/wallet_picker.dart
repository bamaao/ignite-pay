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
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';

import 'package:ignite_pay_app/services/native_deep_link_wallet_service.dart';
import 'package:ignite_pay_app/services/native_wallet_config.dart';
import 'package:ignite_pay_app/services/reown_wallet_service.dart';
import 'package:ignite_pay_app/services/wallet_service.dart';

const _kSurfaceDark = Color(0xFF1A1A2E);
const _kTextSecondary = Color(0xFF8A8AA0);

/// Pick a wallet: native deep link (mobile) or WalletConnect (desktop / QR).
Future<WalletService> selectWalletService(BuildContext context) async {
  if (supportsNativeWalletDeepLink) {
    return _selectNativeWallet(context);
  }
  return ReownWalletService();
}

Future<WalletService> _selectNativeWallet(BuildContext context) async {
  final choice = await showModalBottomSheet<NativeWalletId?>(
    context: context,
    backgroundColor: _kSurfaceDark,
    shape: const RoundedRectangleBorder(
      borderRadius: BorderRadius.vertical(top: Radius.circular(20)),
    ),
    builder: (ctx) => SafeArea(
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'SELECT WALLET',
              style: GoogleFonts.inter(
                fontSize: 11,
                fontWeight: FontWeight.w600,
                color: _kTextSecondary,
                letterSpacing: 1.2,
              ),
            ),
            const SizedBox(height: 16),
            ...NativeWalletConfigs.mobileWallets.map(
              (cfg) => Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: _WalletOptionTile(
                  icon: LucideIcons.link,
                  label: cfg.displayName,
                  subtitle: 'Open installed ${cfg.displayName} app (no QR)',
                  color: cfg.accentColor,
                  onTap: () => Navigator.of(ctx).pop(cfg.id),
                ),
              ),
            ),
            _WalletOptionTile(
              icon: LucideIcons.qrCode,
              label: 'Other Wallets',
              subtitle: 'WalletConnect — scan QR (cross-device)',
              color: const Color(0xFF06B6D4),
              onTap: () => Navigator.of(ctx).pop(null),
            ),
          ],
        ),
      ),
    ),
  );

  if (choice == null) {
    return ReownWalletService();
  }
  return NativeDeepLinkWalletService.forWallet(choice);
}

class _WalletOptionTile extends StatelessWidget {
  final IconData icon;
  final String label;
  final String subtitle;
  final Color color;
  final VoidCallback onTap;

  const _WalletOptionTile({
    required this.icon,
    required this.label,
    required this.subtitle,
    required this.color,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    return Material(
      color: const Color(0xFF12121F),
      borderRadius: BorderRadius.circular(12),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.all(14),
          child: Row(
            children: [
              Container(
                width: 40,
                height: 40,
                decoration: BoxDecoration(
                  color: color.withValues(alpha: 0.15),
                  borderRadius: BorderRadius.circular(10),
                ),
                child: Icon(icon, color: color, size: 20),
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      label,
                      style: GoogleFonts.inter(
                        fontSize: 15,
                        fontWeight: FontWeight.w600,
                        color: Colors.white,
                      ),
                    ),
                    Text(
                      subtitle,
                      style: GoogleFonts.inter(
                        fontSize: 12,
                        color: _kTextSecondary,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
