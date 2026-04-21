import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/merchant_service.dart';
import 'package:ignite_pay_merchant/widgets/order_card.dart';
import 'package:ignite_pay_merchant/payment_detail_screen.dart';
import 'package:provider/provider.dart';

enum PaymentFilter { all, pending, confirmed }

class PaymentListScreen extends StatefulWidget {
  const PaymentListScreen({super.key});

  @override
  State<PaymentListScreen> createState() => _PaymentListScreenState();
}

class _PaymentListScreenState extends State<PaymentListScreen> {
  PaymentFilter _filter = PaymentFilter.all;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      context.read<MerchantService>().refreshOrders();
    });
  }

  List<PaymentOrder> _filtered(List<PaymentOrder> orders) {
    switch (_filter) {
      case PaymentFilter.pending:
        return orders.where((o) => o.status == 'pending').toList();
      case PaymentFilter.confirmed:
        return orders.where((o) => o.status == 'confirmed').toList();
      case PaymentFilter.all:
        return orders;
    }
  }

  @override
  Widget build(BuildContext context) {
    final svc = context.watch<MerchantService>();
    final filtered = _filtered(svc.orders);

    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('收款明细',
                  style: GoogleFonts.inter(
                    fontSize: 20, fontWeight: FontWeight.w700,
                    color: kTextPrimary, letterSpacing: -0.3,
                  )),
              const SizedBox(height: 16),
              // Filter tabs
              SizedBox(
                width: double.infinity,
                child: SegmentedButton<PaymentFilter>(
                  segments: const [
                    ButtonSegment(value: PaymentFilter.all, label: Text('全部')),
                    ButtonSegment(value: PaymentFilter.pending, label: Text('待确认')),
                    ButtonSegment(value: PaymentFilter.confirmed, label: Text('已确认')),
                  ],
                  selected: {_filter},
                  onSelectionChanged: (v) => setState(() => _filter = v.first),
                  style: ButtonStyle(
                    visualDensity: VisualDensity.compact,
                    backgroundColor: WidgetStateProperty.resolveWith((states) {
                      if (states.contains(WidgetState.selected)) {
                        return kNeonCyan.withValues(alpha: 0.15);
                      }
                      return kSurfaceDark;
                    }),
                    foregroundColor: WidgetStateProperty.resolveWith((states) {
                      if (states.contains(WidgetState.selected)) return kNeonCyan;
                      return kTextSecondary;
                    }),
                    side: WidgetStateProperty.all(BorderSide(color: kBorder)),
                    shape: WidgetStateProperty.all(
                      RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
                    ),
                  ),
                ),
              ),
              const SizedBox(height: 12),
              // Order list
              Expanded(
                child: RefreshIndicator(
                  color: kNeonCyan,
                  backgroundColor: kSurfaceDark,
                  onRefresh: () => svc.refreshOrders(),
                  child: filtered.isEmpty
                      ? ListView(children: [
                          SizedBox(height: MediaQuery.of(context).size.height * 0.3),
                          Center(
                            child: Column(
                              children: [
                                Icon(LucideIcons.inbox, size: 36, color: kTextTertiary),
                                const SizedBox(height: 8),
                                Text('暂无收款记录',
                                    style: GoogleFonts.inter(fontSize: 13, color: kTextTertiary)),
                              ],
                            ),
                          ),
                        ])
                      : ListView.builder(
                          itemCount: filtered.length,
                          itemBuilder: (_, i) => Padding(
                            padding: const EdgeInsets.only(bottom: 8),
                            child: OrderCard(
                              order: filtered[i],
                              onTap: () => openPaymentDetail(context, filtered[i]),
                            ),
                          ),
                        ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
