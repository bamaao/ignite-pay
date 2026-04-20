import 'package:flutter/foundation.dart';

/// Payment QR data parsed from a merchant QR code.
class PaymentQrData {
  final String merchantDid;
  final int amount;
  final String description;
  final String orderId;
  final String hubEndpoint;
  final int timestamp;

  PaymentQrData({
    required this.merchantDid,
    required this.amount,
    required this.description,
    required this.orderId,
    required this.hubEndpoint,
    required this.timestamp,
  });
}

/// Channel info from local storage.
class ChannelInfo {
  final String channelId;
  final String hubEndpoint;
  final String userPubkey;
  final String providerPubkey;
  final String status;
  final int sequence;
  final int balance;
  final int totalDeposited;
  final int treeDepth;

  ChannelInfo({
    required this.channelId,
    required this.hubEndpoint,
    required this.userPubkey,
    required this.providerPubkey,
    required this.status,
    required this.sequence,
    required this.balance,
    required this.totalDeposited,
    required this.treeDepth,
  });
}

/// Payment result from a channel payment.
class ChannelPaymentResult {
  final String channelId;
  final int sequence;
  final int leafIndex;
  final String newRoot;

  ChannelPaymentResult({
    required this.channelId,
    required this.sequence,
    required this.leafIndex,
    required this.newRoot,
  });
}

/// Service for state channel operations.
/// Uses the Rust bridge functions for all operations.
class ChannelService extends ChangeNotifier {
  List<ChannelInfo> _channels = [];
  PaymentQrData? _pendingPayment;

  List<ChannelInfo> get channels => _channels;
  PaymentQrData? get pendingPayment => _pendingPayment;

  /// Parse a QR code string into payment data.
  /// This calls the Rust bridge function.
  Future<PaymentQrData> parsePaymentQr(String qrData) async {
    // The Rust bridge function parse_payment_qr will be available
    // after running flutter_rust_bridge_codegen generate
    // For now, we use a placeholder that will be replaced by the bridge
    throw UnimplementedError(
      'Run flutter_rust_bridge_codegen generate first, then use the generated bridge.',
    );
  }

  /// Set a pending payment from a QR scan (for the confirmation screen).
  void setPendingPayment(PaymentQrData data) {
    _pendingPayment = data;
    notifyListeners();
  }

  /// Clear the pending payment.
  void clearPendingPayment() {
    _pendingPayment = null;
    notifyListeners();
  }

  /// Refresh the channel list from local storage.
  Future<void> refreshChannels(String storagePath) async {
    // Will use the Rust bridge after code generation
    notifyListeners();
  }

  /// Find the first open channel, if any.
  ChannelInfo? get openChannel {
    try {
      return _channels.firstWhere(
        (c) => c.status == 'Open' || c.status == 'open',
      );
    } catch (_) {
      return null;
    }
  }

  /// Get total balance across all open channels.
  int get totalBalance {
    return _channels
        .where((c) => c.status == 'Open' || c.status == 'open')
        .fold(0, (sum, c) => sum + c.balance);
  }

  /// Format amount for display (assumes 9 decimal places like SOL lamports).
  static String formatAmount(int amount) {
    return (amount / 1000000000).toStringAsFixed(2);
  }
}
