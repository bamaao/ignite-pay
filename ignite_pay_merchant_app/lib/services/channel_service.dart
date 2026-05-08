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
import 'package:path_provider/path_provider.dart';
import 'package:ignite_pay_merchant/src/rust/api/merchant.dart' as rust;

class ChannelInfo {
  final String channelId;
  final String status;
  final BigInt sequence;
  final int leafCount;
  final BigInt providerBalance;
  final BigInt totalDeposited;

  ChannelInfo({
    required this.channelId,
    required this.status,
    required this.sequence,
    required this.leafCount,
    required this.providerBalance,
    required this.totalDeposited,
  });
}

class ChannelService extends ChangeNotifier {
  List<String> _channelIds = [];
  List<ChannelInfo> _channels = [];
  String _storagePath = '';
  bool _loading = false;

  List<ChannelInfo> get channels => _channels;
  bool get loading => _loading;

  Future<void> initialize() async {
    final dir = await getApplicationSupportDirectory();
    _storagePath = dir.path;
  }

  Future<void> refreshChannels() async {
    _loading = true;
    notifyListeners();
    try {
      _channelIds = await rust.merchantListChannels(storagePath: _storagePath);
      _channels = [];
      for (final id in _channelIds) {
        try {
          final status = await rust.merchantGetChannelStatus(
            storagePath: _storagePath,
            channelId: id,
          );
          _channels.add(ChannelInfo(
            channelId: status.channelId,
            status: status.status,
            sequence: status.sequence,
            leafCount: status.leafCount,
            providerBalance: status.providerBalance,
            totalDeposited: status.totalDeposited,
          ));
        } catch (_) {
          _channels.add(ChannelInfo(
            channelId: id,
            status: 'Unknown',
            sequence: BigInt.zero,
            leafCount: 0,
            providerBalance: BigInt.zero,
            totalDeposited: BigInt.zero,
          ));
        }
      }
    } catch (_) {}
    _loading = false;
    notifyListeners();
  }

  Future<String> closeChannel(String channelId, String hubEndpoint) async {
    return await rust.merchantCloseChannel(
      storagePath: _storagePath,
      hubEndpoint: hubEndpoint,
      channelId: channelId,
    );
  }

  Future<String> claimLeaf(String channelId, String hubEndpoint, int leafIndex, BigInt amount) async {
    return await rust.merchantClaimLeaf(
      storagePath: _storagePath,
      hubEndpoint: hubEndpoint,
      channelId: channelId,
      leafIndex: leafIndex,
      amount: amount,
    );
  }

  Future<String> finalize(String channelId, String hubEndpoint) async {
    return await rust.merchantFinalize(
      storagePath: _storagePath,
      hubEndpoint: hubEndpoint,
      channelId: channelId,
    );
  }
}
