import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';

const _kBackground = Color(0xFF0F0F1A);
const _kNeonCyan = Color(0xFF00F5FF);
const _kTextPrimary = Color(0xFFE8E8F0);
const _kTextSecondary = Color(0xFF8A8AA0);
const _kSuccess = Color(0xFF00E676);
const _kIntercepted = Color(0xFFFF5252);

/// Opens the QR scanner as a full-screen modal.
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

    final url = barcode.rawValue!;
    if (!url.startsWith('didcomm://')) return;

    setState(() {
      _isProcessing = true;
      _error = null;
    });

    try {
      final didService = DidcommService();
      final mcpDid = await didService.parseInvitationAndConnect(url);

      if (mounted) {
        // Show success feedback
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: _kSuccess,
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
      backgroundColor: _kBackground,
      appBar: AppBar(
        backgroundColor: _kBackground,
        elevation: 0,
        leading: IconButton(
          icon: const Icon(LucideIcons.x, color: _kTextPrimary),
          onPressed: () => Navigator.of(context).pop(),
        ),
        title: Text(
          'Scan Pairing QR',
          style: GoogleFonts.inter(
            fontSize: 18,
            fontWeight: FontWeight.w600,
            color: _kTextPrimary,
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
                            ? _kSuccess.withValues(alpha: 0.6)
                            : _kNeonCyan.withValues(alpha: 0.6),
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
                        color: _isProcessing ? _kSuccess : _kNeonCyan,
                      ),
                    ),
                  ),
                ),
                if (_isProcessing)
                  const Center(
                    child: CircularProgressIndicator(color: _kSuccess),
                  ),
              ],
            ),
          ),
          Container(
            padding: const EdgeInsets.all(24),
            child: Column(
              children: [
                Text(
                  'Point the camera at the MCP pairing QR code',
                  style: GoogleFonts.inter(
                    fontSize: 14,
                    color: _kTextSecondary,
                  ),
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: 16),
                if (_error != null)
                  Container(
                    width: double.infinity,
                    padding: const EdgeInsets.all(12),
                    decoration: BoxDecoration(
                      color: _kIntercepted.withValues(alpha: 0.12),
                      borderRadius: BorderRadius.circular(10),
                      border: Border.all(color: _kIntercepted.withValues(alpha: 0.3)),
                    ),
                    child: Text(
                      _error!,
                      style: GoogleFonts.inter(
                        fontSize: 12,
                        color: _kIntercepted,
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
