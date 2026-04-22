import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/merchant_service.dart';
import 'package:provider/provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------
void openNotificationCenter(BuildContext context) {
  Navigator.of(context).push(
    PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 350),
      pageBuilder: (_, animation, _) => SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(1, 0),
          end: Offset.zero,
        ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
        child: const NotificationCenterScreen(),
      ),
    ),
  );
}

// ---------------------------------------------------------------------------
// Notification Center Screen
// ---------------------------------------------------------------------------
class NotificationCenterScreen extends StatefulWidget {
  const NotificationCenterScreen({super.key});

  @override
  State<NotificationCenterScreen> createState() => _NotificationCenterScreenState();
}

class _NotificationCenterScreenState extends State<NotificationCenterScreen> {
  Set<String> _readIds = {};
  static const _prefsKey = 'merchant_read_notification_ids';

  @override
  void initState() {
    super.initState();
    _loadReadIds();
  }

  Future<void> _loadReadIds() async {
    final prefs = await SharedPreferences.getInstance();
    final ids = prefs.getStringList(_prefsKey)?.toSet() ?? <String>{};
    if (mounted) setState(() => _readIds = ids);
  }

  Future<void> _markAsRead(String id) async {
    if (_readIds.contains(id)) return;
    setState(() => _readIds.add(id));
    final prefs = await SharedPreferences.getInstance();
    await prefs.setStringList(_prefsKey, _readIds.toList());
  }

  Future<void> _markAllAsRead(List<String> ids) async {
    setState(() => _readIds.addAll(ids));
    final prefs = await SharedPreferences.getInstance();
    await prefs.setStringList(_prefsKey, _readIds.toList());
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: kBackground,
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 12, 20, 0),
              child: const PageHeader(
                title: '通知中心',
                subtitle: '收款与系统通知',
              ),
            ),
            const SizedBox(height: 16),
            // Mark all as read
            Consumer<MerchantService>(
              builder: (context, svc, _) {
                final notifications = _buildNotifications(svc);
                final unreadCount = notifications
                    .where((n) => !_readIds.contains(n.id))
                    .length;
                if (unreadCount == 0) return const SizedBox.shrink();
                return Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 20),
                  child: GestureDetector(
                    onTap: () => _markAllAsRead(
                      notifications.map((n) => n.id).toList(),
                    ),
                    child: Text(
                      '全部标为已读',
                      style: GoogleFonts.inter(
                        fontSize: 12,
                        fontWeight: FontWeight.w600,
                        color: kNeonCyan,
                      ),
                    ),
                  ),
                );
              },
            ),
            const SizedBox(height: 12),
            // Notification list
            Expanded(
              child: Consumer<MerchantService>(
                builder: (context, svc, _) {
                  final notifications = _buildNotifications(svc);

                  if (notifications.isEmpty) {
                    return Center(
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Icon(LucideIcons.bellOff, size: 40, color: kTextTertiary),
                          const SizedBox(height: 14),
                          Text(
                            '暂无通知',
                            style: GoogleFonts.inter(
                              fontSize: 15,
                              fontWeight: FontWeight.w600,
                              color: kTextSecondary,
                            ),
                          ),
                        ],
                      ),
                    );
                  }

                  return ListView.separated(
                    padding: const EdgeInsets.symmetric(horizontal: 20),
                    itemCount: notifications.length,
                    separatorBuilder: (_, __) => const SizedBox(height: 6),
                    itemBuilder: (context, index) {
                      final n = notifications[index];
                      final isUnread = !_readIds.contains(n.id);
                      return _NotificationTile(
                        notification: n,
                        isUnread: isUnread,
                        onTap: () {
                          _markAsRead(n.id);
                          showDialog(
                            context: context,
                            builder: (ctx) => _NotificationDetailDialog(notification: n),
                          );
                        },
                      );
                    },
                  );
                },
              ),
            ),
            const SizedBox(height: 20),
          ],
        ),
      ),
    );
  }

  List<_MerchantNotification> _buildNotifications(MerchantService svc) {
    // Convert confirmed orders into notifications (most recent first)
    return svc.orders.map((order) {
      final isConfirmed = order.status == 'confirmed';
      final amountDisplay = '${(order.amount.toDouble() / 1e9).toStringAsFixed(4)} SOL';
      return _MerchantNotification(
        id: order.orderId,
        title: isConfirmed ? '收款成功' : '待确认',
        subtitle: isConfirmed
            ? '收到 $amountDisplay'
            : '订单 ${_shortenId(order.orderId)}',
        icon: isConfirmed ? LucideIcons.checkCircle2 : LucideIcons.clock,
        iconColor: isConfirmed ? kSuccess : kPending,
        timestamp: order.confirmedAt ?? order.createdAt,
        details: {
          '订单号': order.orderId,
          '金额': amountDisplay,
          '描述': order.description.isEmpty ? '无' : order.description,
          '状态': order.status,
          '通道': order.channelId ?? '无',
        },
      );
    }).toList();
  }

  String _shortenId(String id) {
    if (id.length > 16) return '${id.substring(0, 10)}...${id.substring(id.length - 4)}';
    return id;
  }
}

// ---------------------------------------------------------------------------
// Notification Model
// ---------------------------------------------------------------------------
class _MerchantNotification {
  final String id;
  final String title;
  final String subtitle;
  final IconData icon;
  final Color iconColor;
  final int timestamp;
  final Map<String, String> details;

  const _MerchantNotification({
    required this.id,
    required this.title,
    required this.subtitle,
    required this.icon,
    required this.iconColor,
    required this.timestamp,
    required this.details,
  });
}

// ---------------------------------------------------------------------------
// Notification Tile
// ---------------------------------------------------------------------------
class _NotificationTile extends StatelessWidget {
  final _MerchantNotification notification;
  final bool isUnread;
  final VoidCallback onTap;

  const _NotificationTile({
    required this.notification,
    required this.isUnread,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.all(14),
        decoration: glassDecoration(
          accentBorder: isUnread ? kNeonCyan.withValues(alpha: 0.15) : null,
        ),
        child: Row(
          children: [
            Container(
              width: 40,
              height: 40,
              decoration: BoxDecoration(
                color: notification.iconColor.withValues(alpha: 0.08),
                borderRadius: BorderRadius.circular(10),
              ),
              child: Icon(notification.icon, size: 18, color: notification.iconColor),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    notification.title,
                    style: GoogleFonts.inter(
                      fontSize: 13,
                      fontWeight: FontWeight.w600,
                      color: kTextPrimary,
                    ),
                  ),
                  const SizedBox(height: 3),
                  Text(
                    notification.subtitle,
                    style: GoogleFonts.inter(
                      fontSize: 11,
                      color: kTextTertiary,
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ],
              ),
            ),
            if (isUnread)
              Container(
                width: 8,
                height: 8,
                decoration: const BoxDecoration(
                  color: kNeonCyan,
                  shape: BoxShape.circle,
                ),
              ),
            const SizedBox(width: 6),
            Icon(LucideIcons.chevronRight, size: 16, color: kTextTertiary),
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Notification Detail Dialog
// ---------------------------------------------------------------------------
class _NotificationDetailDialog extends StatelessWidget {
  final _MerchantNotification notification;
  const _NotificationDetailDialog({required this.notification});

  @override
  Widget build(BuildContext context) {
    return Dialog(
      backgroundColor: kSurfaceDark,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
      child: Padding(
        padding: const EdgeInsets.all(20),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Text(
                  '通知详情',
                  style: GoogleFonts.inter(
                    fontSize: 16,
                    fontWeight: FontWeight.w700,
                    color: kTextPrimary,
                  ),
                ),
                const Spacer(),
                GestureDetector(
                  onTap: () => Navigator.of(context).pop(),
                  child: Icon(LucideIcons.x, size: 20, color: kTextSecondary),
                ),
              ],
            ),
            const SizedBox(height: 16),
            ...notification.details.entries.map((e) => _field(e.key, e.value)),
          ],
        ),
      ),
    );
  }

  Widget _field(String label, String value) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label.toUpperCase(), style: sectionLabel()),
          const SizedBox(height: 2),
          Text(value, style: monoValue(12)),
        ],
      ),
    );
  }
}
