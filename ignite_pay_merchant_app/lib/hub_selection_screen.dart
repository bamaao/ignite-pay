import 'package:flutter/material.dart';
import 'package:ignite_pay_merchant/src/rust/api/merchant_didcomm.dart' as rust;

class HubSelectionScreen extends StatefulWidget {
  final String registryUrl;
  final String storagePath;
  final String mcpDid;

  const HubSelectionScreen({
    super.key,
    required this.registryUrl,
    required this.storagePath,
    required this.mcpDid,
  });

  @override
  State<HubSelectionScreen> createState() => _HubSelectionScreenState();
}

class _HubSelectionScreenState extends State<HubSelectionScreen> {
  List<rust.HubInfo> _hubs = [];
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _loadHubs();
  }

  Future<void> _loadHubs() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final hubs = await rust.fetchHubList(registryUrl: widget.registryUrl);
      if (mounted) {
        setState(() {
          _hubs = hubs;
          _loading = false;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = e.toString();
          _loading = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFF0A0A14),
      appBar: AppBar(
        backgroundColor: const Color(0xFF0A0A14),
        title: const Text(
          '\u9009\u62e9 Hub',
          style: TextStyle(color: Color(0xFF00F5FF), fontSize: 18),
        ),
        iconTheme: const IconThemeData(color: Color(0xFF00F5FF)),
        elevation: 0,
      ),
      body: _loading
          ? const Center(
              child: CircularProgressIndicator(color: Color(0xFF00F5FF)),
            )
          : _error != null
              ? Center(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      const Icon(Icons.error_outline, color: Color(0xFFFF5252), size: 48),
                      const SizedBox(height: 12),
                      Text(
                        '\u52a0\u8f7d\u5931\u8d25',
                        style: const TextStyle(color: Color(0xFFFF5252), fontSize: 16),
                      ),
                      const SizedBox(height: 8),
                      Text(
                        _error!,
                        style: const TextStyle(color: Colors.white54, fontSize: 12),
                        textAlign: TextAlign.center,
                      ),
                      const SizedBox(height: 16),
                      ElevatedButton(
                        onPressed: _loadHubs,
                        style: ElevatedButton.styleFrom(
                          backgroundColor: const Color(0xFF00F5FF),
                        ),
                        child: const Text('\u91cd\u8bd5'),
                      ),
                    ],
                  ),
                )
              : _hubs.isEmpty
                  ? const Center(
                      child: Column(
                        mainAxisAlignment: MainAxisAlignment.center,
                        children: [
                          Icon(Icons.layers, color: Colors.white24, size: 64),
                          SizedBox(height: 12),
                          Text(
                            '\u6682\u65e0\u53ef\u7528 Hub',
                            style: TextStyle(color: Colors.white38, fontSize: 16),
                          ),
                        ],
                      ),
                    )
                  : RefreshIndicator(
                      color: const Color(0xFF00F5FF),
                      onRefresh: _loadHubs,
                      child: ListView.builder(
                        padding: const EdgeInsets.all(16),
                        itemCount: _hubs.length,
                        itemBuilder: (context, index) {
                          final hub = _hubs[index];
                          return _MerchantHubCard(
                            hub: hub,
                            onTap: () => _showCreateChannelSheet(hub),
                          );
                        },
                      ),
                    ),
    );
  }

  void _showCreateChannelSheet(rust.HubInfo hub) {
    final depositController = TextEditingController(text: '1000000000');
    final treeDepthController = TextEditingController(text: '8');
    final tokenMintController = TextEditingController(
      text: 'So11111111111111111111111111111111',
    );

    showModalBottomSheet(
      context: context,
      backgroundColor: const Color(0xFF1A1A2E),
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(20)),
      ),
      builder: (context) {
        return Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                '\u521b\u5efa\u901a\u9053',
                style: const TextStyle(
                  color: Color(0xFF00F5FF),
                  fontSize: 18,
                  fontWeight: FontWeight.bold,
                ),
              ),
              const SizedBox(height: 4),
              Text(
                hub.name,
                style: const TextStyle(color: Colors.white70, fontSize: 14),
              ),
              const SizedBox(height: 16),
              TextField(
                controller: depositController,
                keyboardType: TextInputType.number,
                style: const TextStyle(color: Colors.white),
                decoration: const InputDecoration(
                  labelText: 'Deposit Amount (lamports)',
                  labelStyle: TextStyle(color: Colors.white54),
                  enabledBorder: UnderlineInputBorder(
                    borderSide: BorderSide(color: Colors.white24),
                  ),
                ),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: treeDepthController,
                keyboardType: TextInputType.number,
                style: const TextStyle(color: Colors.white),
                decoration: const InputDecoration(
                  labelText: 'Tree Depth',
                  labelStyle: TextStyle(color: Colors.white54),
                  enabledBorder: UnderlineInputBorder(
                    borderSide: BorderSide(color: Colors.white24),
                  ),
                ),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: tokenMintController,
                style: const TextStyle(color: Colors.white),
                decoration: const InputDecoration(
                  labelText: 'Token Mint',
                  labelStyle: TextStyle(color: Colors.white54),
                  enabledBorder: UnderlineInputBorder(
                    borderSide: BorderSide(color: Colors.white24),
                  ),
                ),
              ),
              const SizedBox(height: 24),
              SizedBox(
                width: double.infinity,
                child: ElevatedButton(
                  onPressed: () => _createChannel(
                    context,
                    hub,
                    depositController.text,
                    treeDepthController.text,
                    tokenMintController.text,
                  ),
                  style: ElevatedButton.styleFrom(
                    backgroundColor: const Color(0xFF00F5FF),
                    foregroundColor: const Color(0xFF0A0A14),
                    padding: const EdgeInsets.symmetric(vertical: 14),
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(12),
                    ),
                  ),
                  child: const Text(
                    '\u786e\u8ba4\u521b\u5efa',
                    style: TextStyle(fontSize: 16, fontWeight: FontWeight.bold),
                  ),
                ),
              ),
            ],
          ),
        );
      },
    );
  }

  Future<void> _createChannel(
    BuildContext sheetContext,
    rust.HubInfo hub,
    String depositStr,
    String treeDepthStr,
    String tokenMint,
  ) async {
    final deposit = BigInt.tryParse(depositStr) ?? BigInt.zero;
    final treeDepth = int.tryParse(treeDepthStr) ?? 8;

    Navigator.of(sheetContext).pop();

    try {
      await rust.sendCreateChannelRequest(
        storagePath: widget.storagePath,
        mcpDid: widget.mcpDid,
        hubEndpoint: hub.endpointUrl,
        providerPubkey: hub.hubDid.replaceFirst('did:ignite:', ''),
        tokenMint: tokenMint,
        deposit: deposit,
        treeDepth: treeDepth,
      );

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('\u521b\u5efa\u901a\u9053\u8bf7\u6c42\u5df2\u53d1\u9001'),
            backgroundColor: Color(0xFF00E676),
          ),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('\u521b\u5efa\u5931\u8d25: $e'),
            backgroundColor: const Color(0xFFFF5252),
          ),
        );
      }
    }
  }
}

class _MerchantHubCard extends StatelessWidget {
  final rust.HubInfo hub;
  final VoidCallback onTap;

  const _MerchantHubCard({required this.hub, required this.onTap});

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.only(bottom: 12),
      decoration: BoxDecoration(
        gradient: const LinearGradient(
          colors: [Color(0xFF1A1A2E), Color(0xFF12121F)],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: const Color(0xFF8B5CF6).withValues(alpha: 0.3)),
      ),
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          borderRadius: BorderRadius.circular(12),
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Expanded(
                      child: Text(
                        hub.name,
                        style: const TextStyle(
                          color: Colors.white,
                          fontSize: 16,
                          fontWeight: FontWeight.w600,
                        ),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                    Container(
                      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                      decoration: BoxDecoration(
                        color: const Color(0xFF00E676).withValues(alpha: 0.2),
                        borderRadius: BorderRadius.circular(8),
                      ),
                      child: Text(
                        '\u5728\u7ebf ${hub.onlineRate}%',
                        style: const TextStyle(
                          color: Color(0xFF00E676),
                          fontSize: 11,
                        ),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                Row(
                  children: [
                    _MetricChip(
                      label: '\u8d39\u7387',
                      value: '${hub.feeRateBps} bps',
                    ),
                    const SizedBox(width: 8),
                    _MetricChip(
                      label: '\u6d41\u52a8\u6027',
                      value: _formatLiquidity(hub.availableLiquidity),
                    ),
                    const SizedBox(width: 8),
                    _MetricChip(
                      label: '\u6210\u529f\u7387',
                      value: '${hub.successRate}%',
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  String _formatLiquidity(BigInt amount) {
    final sol = amount.toDouble() / 1e9;
    if (sol >= 1) {
      return '${sol.toStringAsFixed(1)} SOL';
    }
    return '$amount lam';
  }
}

class _MetricChip extends StatelessWidget {
  final String label;
  final String value;

  const _MetricChip({required this.label, required this.value});

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 3),
      decoration: BoxDecoration(
        color: Colors.white.withValues(alpha: 0.05),
        borderRadius: BorderRadius.circular(6),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            label,
            style: const TextStyle(color: Colors.white38, fontSize: 9),
          ),
          Text(
            value,
            style: const TextStyle(
              color: Color(0xFF8B5CF6),
              fontSize: 11,
              fontFamily: 'JetBrains Mono',
            ),
          ),
        ],
      ),
    );
  }
}
