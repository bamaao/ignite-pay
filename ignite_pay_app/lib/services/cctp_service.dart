import 'dart:async';
import 'package:flutter/foundation.dart';
import 'package:ignite_pay_app/src/rust/api/simple.dart' as bridge;
import 'package:ignite_pay_app/src/rust/api/cctp_transfer.dart';
import 'package:ignite_pay_app/services/evm_wallet_service.dart';

/// State of the CCTP transfer flow.
enum CctpState {
  idle,
  fetchingFees,
  approving,
  burning,
  polling,
  done,
  error,
}

/// CCTP transfer result.
class CctpResult {
  final bool success;
  final String? forwardTxHash;
  final String? error;

  const CctpResult.success({this.forwardTxHash})
      : success = true,
        error = null;
  const CctpResult.failure({required this.error})
      : success = false,
        forwardTxHash = null;
}

/// Orchestrates the CCTP Forwarding cross-chain deposit flow:
///   1. fetchFees() — query Circle Iris API for forwarding fees
///   2. buildApproveCalldata() — build ERC-20 approve calldata
///   3. buildBurnCalldata() — build depositForBurnWithHook calldata
///   4. pollStatus() — poll until transfer completes
class CctpService extends ChangeNotifier {
  static final CctpService _instance = CctpService._internal();
  factory CctpService() => _instance;
  CctpService._internal();

  // ── State ────────────────────────────────────────────────────────────

  CctpState _state = CctpState.idle;
  CctpState get state => _state;

  String? _errorMessage;
  String? get errorMessage => _errorMessage;

  CctpFeeQuote? _feeQuote;
  CctpFeeQuote? get feeQuote => _feeQuote;

  String? _lastBurnTxHash;
  String? get lastBurnTxHash => _lastBurnTxHash;

  String? _forwardTxHash;
  String? get forwardTxHash => _forwardTxHash;

  final _evm = EvmWalletService();

  // ── EVM chain config ─────────────────────────────────────────────────

  /// Supported source chains for CCTP transfers.
  static const List<CctpChainConfig> supportedChains = [
    CctpChainConfig(
      name: 'Ethereum',
      domainId: 0,
      tokenMessenger: '0xBD3fa9AE8AcB092cC21E555769777B85a666E4db',
      usdc: '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
    ),
    CctpChainConfig(
      name: 'Base',
      domainId: 6,
      tokenMessenger: '0x9DAF7a48A68C0c2a88289f3f987a1e8D25d58685',
      usdc: '0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913',
    ),
    CctpChainConfig(
      name: 'Arbitrum',
      domainId: 3,
      tokenMessenger: '0x19330d10D9Cc8751218eaf51E8885D058642E08A',
      usdc: '0xaf88d065e77c8cC2239327C5EDb3A432268e5831',
    ),
    CctpChainConfig(
      name: 'OP',
      domainId: 2,
      tokenMessenger: '0x9DAF7a48A68C0c2a88289f3f987a1e8D25d58685',
      usdc: '0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85',
    ),
  ];

  static const solanaDomainId = 5;

  // ── Fee Quote ────────────────────────────────────────────────────────

  /// Fetch forwarding fees from Circle Iris API.
  Future<CctpFeeQuote> fetchFees({
    required String irisApiUrl,
    required int srcDomain,
  }) async {
    _state = CctpState.fetchingFees;
    _errorMessage = null;
    notifyListeners();

    try {
      _feeQuote = await bridge.cctpGetFees(
        irisApiUrl: irisApiUrl,
        srcDomain: srcDomain,
        dstDomain: solanaDomainId,
      );
      _state = CctpState.idle;
      notifyListeners();
      return _feeQuote!;
    } catch (e) {
      _state = CctpState.error;
      _errorMessage = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  // ── Build Calldata ───────────────────────────────────────────────────

  /// Build ERC-20 approve calldata for USDC → TokenMessengerV2.
  Future<String> buildApproveCalldata({
    required CctpChainConfig chain,
    required int amount,
  }) async {
    return bridge.cctpBuildApproveCalldata(
      spender: chain.tokenMessenger,
      amount: BigInt.from(amount),
    );
  }

  /// Build depositForBurnWithHook calldata.
  Future<String> buildBurnCalldata({
    required CctpChainConfig chain,
    required int amount,
    required String mintRecipientHex,
  }) async {
    return bridge.cctpBuildDepositForBurnCalldata(
      amount: BigInt.from(amount),
      dstDomain: solanaDomainId,
      mintRecipient: mintRecipientHex,
      burnToken: chain.usdc,
      dstCaller: '0' * 64, // any caller
      maxFee: 200,
      minFinalityThreshold: 1000,
    );
  }

  // ── MetaMask Deep Link ───────────────────────────────────────────────

  /// Open MetaMask with approve calldata.
  Future<bool> openApproveTx({
    required CctpChainConfig chain,
    required String approveCalldata,
  }) async {
    _state = CctpState.approving;
    notifyListeners();

    final url = _evm.buildMetaMaskUrl(
      to: chain.usdc,
      data: approveCalldata,
      redirectPath: 'cctp_approve',
    );
    return _evm.openWalletUrl(url);
  }

  /// Open MetaMask with depositForBurnWithHook calldata.
  Future<bool> openBurnTx({
    required CctpChainConfig chain,
    required String burnCalldata,
  }) async {
    _state = CctpState.burning;
    notifyListeners();

    final url = _evm.buildMetaMaskUrl(
      to: chain.tokenMessenger,
      data: burnCalldata,
      redirectPath: 'cctp_burn',
    );
    return _evm.openWalletUrl(url);
  }

  // ── Status Polling ───────────────────────────────────────────────────

  /// Poll Circle Iris API for transfer status with exponential backoff.
  Future<CctpResult> pollStatus({
    required String irisApiUrl,
    required int srcDomain,
    required String burnTxHash,
    Duration initialDelay = const Duration(seconds: 15),
    int maxAttempts = 40,
  }) async {
    _state = CctpState.polling;
    _lastBurnTxHash = burnTxHash;
    _forwardTxHash = null;
    notifyListeners();

    var delay = initialDelay;

    for (var attempt = 0; attempt < maxAttempts; attempt++) {
      try {
        final status = await bridge.cctpPollStatus(
          irisApiUrl: irisApiUrl,
          srcDomain: srcDomain,
          burnTxHash: burnTxHash,
        );

        if (status.state == 'complete') {
          _forwardTxHash = status.forwardTxHash;
          _state = CctpState.done;
          notifyListeners();
          return CctpResult.success(forwardTxHash: status.forwardTxHash);
        }

        if (status.state == 'not_found' && attempt < maxAttempts - 1) {
          // Not seen yet — keep polling
          await Future.delayed(delay);
          delay = Duration(
            milliseconds: (delay.inMilliseconds * 1.5).round().clamp(5000, 120000),
          );
          continue;
        }

        // Non-complete terminal state or last attempt
        if (attempt == maxAttempts - 1) {
          _state = CctpState.error;
          _errorMessage = 'Transfer not completed after $maxAttempts attempts';
          notifyListeners();
          return CctpResult.failure(error: _errorMessage!);
        }

        // Still pending — wait and retry
        await Future.delayed(delay);
        delay = Duration(
          milliseconds: (delay.inMilliseconds * 1.5).round().clamp(5000, 120000),
        );
      } catch (e) {
        if (attempt == maxAttempts - 1) {
          _state = CctpState.error;
          _errorMessage = e.toString();
          notifyListeners();
          return CctpResult.failure(error: e.toString());
        }
        await Future.delayed(delay);
      }
    }

    _state = CctpState.error;
    _errorMessage = 'Transfer polling exhausted';
    notifyListeners();
    return const CctpResult.failure(error: 'Transfer polling exhausted');
  }

  // ── Derive ATA ───────────────────────────────────────────────────────

  /// Derive the Solana USDC ATA for a wallet (returns hex bytes32).
  Future<String> deriveSolanaUsdcAta(String walletB58) async {
    return bridge.cctpDeriveSolanaUsdcAta(walletB58: walletB58);
  }

  // ── Reset ────────────────────────────────────────────────────────────

  /// Clear all state.
  void reset() {
    _state = CctpState.idle;
    _errorMessage = null;
    _feeQuote = null;
    _lastBurnTxHash = null;
    _forwardTxHash = null;
    notifyListeners();
  }
}

/// Configuration for a supported EVM source chain.
class CctpChainConfig {
  final String name;
  final int domainId;
  final String tokenMessenger;
  final String usdc;

  const CctpChainConfig({
    required this.name,
    required this.domainId,
    required this.tokenMessenger,
    required this.usdc,
  });
}
