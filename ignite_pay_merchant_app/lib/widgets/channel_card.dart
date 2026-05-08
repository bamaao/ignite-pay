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
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/channel_service.dart';

class ChannelCard extends StatelessWidget {
  final ChannelInfo channel;
  final VoidCallback? onTap;

  const ChannelCard({super.key, required this.channel, this.onTap});

  @override
  Widget build(BuildContext context) {
    final statusColor = _channelStatusColor(channel.status);
    final shortId = channel.channelId.length > 8
        ? channel.channelId.substring(0, 8)
        : channel.channelId;
    final balance = (channel.providerBalance.toDouble() / 1_000_000_000).toStringAsFixed(2);

    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.all(14),
        decoration: glassDecoration(),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Text('通道 $shortId...', style: cardTitle()),
                const Spacer(),
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                  decoration: BoxDecoration(
                    color: statusColor.withValues(alpha: 0.12),
                    borderRadius: BorderRadius.circular(10),
                    border: Border.all(color: statusColor.withValues(alpha: 0.3)),
                  ),
                  child: Text(
                    channel.status,
                    style: GoogleFonts.inter(
                      fontSize: 10,
                      fontWeight: FontWeight.w600,
                      color: statusColor,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Row(
              children: [
                Text('序列: ${channel.sequence}', style: cardSubtitle()),
                const Spacer(),
                Text('$balance USDC', style: monoValue(13)),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

Color _channelStatusColor(String status) {
  switch (status) {
    case 'Open':
      return kSuccess;
    case 'Closed':
      return kDanger;
    case 'Settling':
      return kPending;
    default:
      return kTextSecondary;
  }
}
