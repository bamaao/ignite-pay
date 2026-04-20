import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'services/channel_service.dart';
import 'theme.dart';

/// QR Payment Confirmation Screen.
///
/// Shown after scanning a merchant's payment QR code.
/// Displays payment details and a confirm/cancel flow.
class QrPaymentScreen extends StatefulWidget {
  final PaymentQrData paymentData;
  final String storagePath;
  final Future<ChannelPaymentResult> Function({
    required String storagePath,
    required String channelId,
    required String hubEndpoint,
    required int amount,
    required String recipientPubkey,
  }) onConfirmPayment;

  const QrPaymentScreen({
    super.key,
    required this.paymentData,
    required this.storagePath,
    required this.onConfirmPayment,
  });

  @override
  State<QrPaymentScreen> createState() => _QrPaymentScreenState();
}

class _QrPaymentScreenState extends State<QrPaymentScreen> {
  bool _isProcessing = false;
  bool _isSuccess = false;
  String? _errorMessage;
  ChannelPaymentResult? _result;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: kBackground,
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            children: [
              // Header
              Row(
                children: [
                  BackButtonGlass(
                    onTap: _isProcessing ? null : () => Navigator.of(context).pop(),
                  ),
                  const Spacer(),
                  Text(
                    '确认支付',
                    style: GoogleFonts.inter(
                      fontSize: 18,
                      fontWeight: FontWeight.w700,
                      color: kTextPrimary,
                    ),
                  ),
                  const Spacer(),
                  const SizedBox(width: 36), // Balance header
                ],
              ),
              const SizedBox(height: 32),

              // Merchant info card
              _buildMerchantCard(),
              const SizedBox(height: 24),

              // Amount
              Text(
                ChannelService.formatAmount(widget.paymentData.amount),
                style: GoogleFonts.jetBrainsMono(
                  fontSize: 36,
                  fontWeight: FontWeight.w700,
                  color: kNeonCyan,
                ),
              ),
              const SizedBox(height: 4),
              Text(
                widget.paymentData.description,
                style: GoogleFonts.inter(
                  fontSize: 14,
                  color: kTextSecondary,
                ),
              ),
              const SizedBox(height: 24),

              // Channel info card
              _buildChannelCard(),
              const Spacer(),

              // Error message
              if (_errorMessage != null) ...[
                Container(
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: kDanger.withValues(alpha: 0.1),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: kDanger.withValues(alpha: 0.3)),
                  ),
                  child: Text(
                    _errorMessage!,
                    style: GoogleFonts.inter(fontSize: 12, color: kDanger),
                  ),
                ),
                const SizedBox(height: 16),
              ],

              // Success message
              if (_isSuccess && _result != null) ...[
                Container(
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: kSuccess.withValues(alpha: 0.1),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: kSuccess.withValues(alpha: 0.3)),
                  ),
                  child: Text(
                    '支付成功! Sequence: ${_result!.sequence}, Leaf: ${_result!.leafIndex}',
                    style: GoogleFonts.inter(fontSize: 12, color: kSuccess),
                  ),
                ),
                const SizedBox(height: 16),
              ],

              // Action buttons
              if (!_isSuccess) ...[
                SizedBox(
                  width: double.infinity,
                  height: 48,
                  child: ElevatedButton(
                    onPressed: _isProcessing ? null : _onConfirm,
                    style: ElevatedButton.styleFrom(
                      backgroundColor: kNeonCyan,
                      foregroundColor: kBackground,
                      shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(12),
                      ),
                    ),
                    child: _isProcessing
                        ? const SizedBox(
                            width: 20,
                            height: 20,
                            child: CircularProgressIndicator(
                              strokeWidth: 2,
                              color: kBackground,
                            ),
                          )
                        : Text(
                            '确认支付',
                            style: GoogleFonts.inter(
                              fontSize: 15,
                              fontWeight: FontWeight.w700,
                            ),
                          ),
                  ),
                ),
                const SizedBox(height: 8),
                SizedBox(
                  width: double.infinity,
                  height: 44,
                  child: TextButton(
                    onPressed: _isProcessing
                        ? null
                        : () => Navigator.of(context).pop(),
                    style: TextButton.styleFrom(
                      foregroundColor: kTextSecondary,
                    ),
                    child: Text(
                      '取消',
                      style: GoogleFonts.inter(fontSize: 14),
                    ),
                  ),
                ),
              ] else ...[
                SizedBox(
                  width: double.infinity,
                  height: 48,
                  child: ElevatedButton(
                    onPressed: () => Navigator.of(context).pop(_result),
                    style: ElevatedButton.styleFrom(
                      backgroundColor: kSuccess,
                      foregroundColor: kBackground,
                      shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(12),
                      ),
                    ),
                    child: Text(
                      '完成',
                      style: GoogleFonts.inter(
                        fontSize: 15,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildMerchantCard() {
    return Container(
      padding: const EdgeInsets.all(16),
      decoration: glassCardDecoration(),
      child: Row(
        children: [
          Container(
            width: 40,
            height: 40,
            decoration: BoxDecoration(
              color: kPurple.withValues(alpha: 0.1),
              borderRadius: BorderRadius.circular(10),
              border: Border.all(color: kPurple.withValues(alpha: 0.2)),
            ),
            child: const Icon(Icons.store, size: 20, color: kPurple),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  '商户',
                  style: GoogleFonts.inter(
                    fontSize: 14,
                    fontWeight: FontWeight.w600,
                    color: kTextPrimary,
                  ),
                ),
                const SizedBox(height: 2),
                Text(
                  _shortenDid(widget.paymentData.merchantDid),
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 11,
                    color: kTextSecondary,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildChannelCard() {
    return Container(
      padding: const EdgeInsets.all(16),
      decoration: glassDecoration(),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '支付通道',
            style: GoogleFonts.inter(
              fontSize: 10,
              fontWeight: FontWeight.w700,
              color: kTextTertiary,
              letterSpacing: 1.5,
            ),
          ),
          const SizedBox(height: 8),
          Row(
            children: [
              Icon(
                Icons.hub,
                size: 14,
                color: kNeonCyanDim,
              ),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  'Hub: ${widget.paymentData.hubEndpoint.replaceAll(RegExp(r'https?://'), '')}',
                  style: GoogleFonts.inter(
                    fontSize: 12,
                    color: kTextSecondary,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }

  String _shortenDid(String did) {
    if (did.length <= 24) return did;
    return '${did.substring(0, 16)}...${did.substring(did.length - 8)}';
  }

  Future<void> _onConfirm() async {
    setState(() {
      _isProcessing = true;
      _errorMessage = null;
    });

    try {
      final result = await widget.onConfirmPayment(
        storagePath: widget.storagePath,
        channelId: '', // Will be determined by available open channel
        hubEndpoint: widget.paymentData.hubEndpoint,
        amount: widget.paymentData.amount,
        recipientPubkey: widget.paymentData.merchantDid,
      );

      if (mounted) {
        setState(() {
          _isProcessing = false;
          _isSuccess = true;
          _result = result;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _isProcessing = false;
          _errorMessage = e.toString();
        });
      }
    }
  }
}
