import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/merchant_push_service.dart';

/// Opens the QR scanner as a full-screen modal.
/// Returns the MCP DID string on success, or null if cancelled.
Future<String?> showQrScanner(BuildContext context) {
  return Navigator.of(context).push<String>(
    MaterialPageRoute(
      builder: (_) => const _QrScannerScreen(),
      fullscreenDialog: true,
    ),
  );
}

class _QrScannerScreen extends StatefulWidget {
  const _QrScannerScreen();

  @override
  State<_QrScannerScreen> createState() => _QrScannerScreenState();
}

class _QrScannerScreenState extends State<_QrScannerScreen> {
  final MobileScannerController _scannerController = MobileScannerController();
  bool _isProcessing = false;
  String? _error;

  @override
  void dispose() {
    _scannerController.dispose();
    super.dispose();
  }

  Future<void> _onDetect(BarcodeCapture capture) async {
    if (_isProcessing) return;
    if (capture.barcodes.isEmpty) return;
    final barcode = capture.barcodes.first;
    if (barcode.rawValue == null) return;

    final rawValue = barcode.rawValue!;

    // Only handle didcomm:// pairing QR
    if (!rawValue.startsWith('didcomm://')) {
      setState(() {
        _error = 'Not a valid pairing QR code. Expected didcomm:// prefix.';
      });
      return;
    }

    setState(() {
      _isProcessing = true;
      _error = null;
    });

    try {
      final pushSvc = MerchantPushService();
      final mcpDid = await pushSvc.parseInvitationAndConnect(rawValue);

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kSuccess,
            behavior: SnackBarBehavior.floating,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text(
              'Paired with $mcpDid',
              style: GoogleFonts.inter(fontWeight: FontWeight.w600),
            ),
            duration: const Duration(seconds: 2),
          ),
        );
        Navigator.of(context).pop(mcpDid);
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _isProcessing = false;
          _error = e.toString();
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: kBackground,
      appBar: AppBar(
        backgroundColor: kBackground,
        elevation: 0,
        leading: IconButton(
          icon: const Icon(LucideIcons.x, color: kTextPrimary),
          onPressed: () => Navigator.of(context).pop(),
        ),
        title: Text(
          'Scan Pairing QR',
          style: GoogleFonts.inter(
            fontSize: 18,
            fontWeight: FontWeight.w600,
            color: kTextPrimary,
          ),
        ),
      ),
      body: Column(
        children: [
          Expanded(
            child: Stack(
              children: [
                MobileScanner(
                  controller: _scannerController,
                  onDetect: _onDetect,
                ),
                // Scan area overlay
                Center(
                  child: Container(
                    width: 260,
                    height: 260,
                    decoration: BoxDecoration(
                      border: Border.all(
                        color: _isProcessing
                            ? kSuccess.withValues(alpha: 0.6)
                            : kNeonCyan.withValues(alpha: 0.6),
                        width: 3,
                      ),
                      borderRadius: BorderRadius.circular(20),
                    ),
                  ),
                ),
                // Corner indicators
                Center(
                  child: SizedBox(
                    width: 260,
                    height: 260,
                    child: CustomPaint(
                      painter: _CornerPainter(
                        color: _isProcessing ? kSuccess : kNeonCyan,
                      ),
                    ),
                  ),
                ),
                if (_isProcessing)
                  const Center(
                    child: CircularProgressIndicator(color: kSuccess),
                  ),
              ],
            ),
          ),
          Container(
            padding: const EdgeInsets.all(24),
            child: Column(
              children: [
                Text(
                  'Point the camera at an MCP pairing QR code',
                  style: GoogleFonts.inter(
                    fontSize: 14,
                    color: kTextSecondary,
                  ),
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: 16),
                if (_error != null)
                  Container(
                    width: double.infinity,
                    padding: const EdgeInsets.all(12),
                    decoration: BoxDecoration(
                      color: kDanger.withValues(alpha: 0.12),
                      borderRadius: BorderRadius.circular(10),
                      border: Border.all(color: kDanger.withValues(alpha: 0.3)),
                    ),
                    child: Text(
                      _error!,
                      style: GoogleFonts.inter(
                        fontSize: 12,
                        color: kDanger,
                      ),
                      textAlign: TextAlign.center,
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// Paints corner indicators around the scan area.
class _CornerPainter extends CustomPainter {
  final Color color;
  _CornerPainter({required this.color});

  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..color = color
      ..strokeWidth = 4
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round;

    const len = 24.0;
    const r = 20.0;

    // Top-left
    canvas.drawLine(const Offset(r, r + len), const Offset(r, r), paint);
    canvas.drawLine(const Offset(r, r), const Offset(r + len, r), paint);

    // Top-right
    canvas.drawLine(Offset(size.width - r - len, r), Offset(size.width - r, r), paint);
    canvas.drawLine(Offset(size.width - r, r), Offset(size.width - r, r + len), paint);

    // Bottom-left
    canvas.drawLine(Offset(r, size.height - r - len), Offset(r, size.height - r), paint);
    canvas.drawLine(Offset(r, size.height - r), Offset(r + len, size.height - r), paint);

    // Bottom-right
    canvas.drawLine(
        Offset(size.width - r, size.height - r), Offset(size.width - r - len, size.height - r), paint);
    canvas.drawLine(
        Offset(size.width - r, size.height - r - len), Offset(size.width - r, size.height - r), paint);
  }

  @override
  bool shouldRepaint(covariant _CornerPainter old) => old.color != color;
}
