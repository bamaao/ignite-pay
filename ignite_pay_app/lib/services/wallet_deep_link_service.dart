import 'package:url_launcher/url_launcher.dart';

/// Service for constructing and opening wallet deep links (Phantom, Solflare).
class WalletDeepLinkService {
  static final WalletDeepLinkService _instance = WalletDeepLinkService._internal();
  factory WalletDeepLinkService() => _instance;
  WalletDeepLinkService._internal();

  /// Build a Phantom deep link URL for signing and sending a transaction.
  /// Returns the full URL to open via url_launcher.
  String buildPhantomSignUrl({
    required String transactionB58,
    required String redirectScheme,
    required String redirectPath,
  }) {
    // Phantom v1 signAndSendTransaction deep link
    // In a production app, this would use proper encryption with dApp keypair.
    // For now, this constructs the URL with the transaction payload in cleartext
    // (devnet/testing only).
    final redirect = Uri.encodeFull('$redirectScheme://$redirectPath');
    return 'https://phantom.app/ul/v1/signAndSendTransaction'
        '?dapp_encryption_public_key=placeholder'
        '&payload=${Uri.encodeComponent(transactionB58)}'
        '&redirect_link=$redirect'
        '&cluster=devnet';
  }

  /// Build a Solflare deep link URL for signing and sending a transaction.
  String buildSolflareSignUrl({
    required String transactionB58,
    required String redirectScheme,
    required String redirectPath,
  }) {
    final redirect = Uri.encodeFull('$redirectScheme://$redirectPath');
    return 'solflare://v1/signAndSendTransaction'
        '?dapp_encryption_public_key=placeholder'
        '&payload=${Uri.encodeComponent(transactionB58)}'
        '&redirect_link=$redirect'
        '&cluster=devnet';
  }

  /// Open a deep link URL in the wallet app.
  Future<bool> openWalletUrl(String url) async {
    final uri = Uri.parse(url);
    if (await canLaunchUrl(uri)) {
      return launchUrl(uri, mode: LaunchMode.externalApplication);
    }
    return false;
  }

  /// Parse a deep link callback to extract the transaction signature.
  /// Expected format: ignitepay://onchain?signature=...
  String? parseCallbackSignature(String callbackUrl) {
    final uri = Uri.parse(callbackUrl);
    return uri.queryParameters['signature'];
  }

  /// Parse a deep link callback to extract an error.
  String? parseCallbackError(String callbackUrl) {
    final uri = Uri.parse(callbackUrl);
    return uri.queryParameters['errorCode'] ?? uri.queryParameters['errorMessage'];
  }
}
