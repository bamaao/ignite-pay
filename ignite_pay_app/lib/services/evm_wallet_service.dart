import 'package:url_launcher/url_launcher.dart';
import 'package:flutter/services.dart';

/// Service for constructing and opening MetaMask deep links for EVM transactions.
class EvmWalletService {
  static final EvmWalletService _instance = EvmWalletService._internal();
  factory EvmWalletService() => _instance;
  EvmWalletService._internal();

  /// Build a MetaMask deep link URL for sending a contract transaction.
  ///
  /// Format: `https://metamask.app.link/send/{to}?data={data}&value={value}`
  /// The `redirect` parameter controls where MetaMask returns after the tx.
  String buildMetaMaskUrl({
    required String to,
    required String data,
    String value = '0',
    String redirectScheme = 'ignitepay',
    String redirectPath = 'cctp_callback',
  }) {
    final redirect = Uri.encodeFull('$redirectScheme://$redirectPath');
    return 'https://metamask.app.link/send/$to'
        '?data=${Uri.encodeComponent(data)}'
        '&value=$value'
        '&redirect=$redirect';
  }

  /// Open a wallet URL via url_launcher.
  Future<bool> openWalletUrl(String url) async {
    final uri = Uri.parse(url);
    if (await canLaunchUrl(uri)) {
      return launchUrl(uri, mode: LaunchMode.externalApplication);
    }
    return false;
  }

  /// Copy calldata hex to clipboard for manual wallet operation.
  Future<void> copyToClipboard(String data) async {
    await Clipboard.setData(ClipboardData(text: data));
  }
}
