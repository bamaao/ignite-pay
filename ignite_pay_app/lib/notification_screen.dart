import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/theme.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
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

  static const _prefsKey = 'read_notification_ids';

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
                title: 'Notifications',
                subtitle: 'System & connection alerts',
              ),
            ),
            const SizedBox(height: 16),
            // Mark all as read button
            Consumer<DidcommService>(
              builder: (context, svc, _) {
                final notifications = svc.messages
                    .where((m) => !m.msgType.contains('payment-auth-request'))
                    .toList();
                final unreadCount = notifications
                    .where((m) => !_readIds.contains(m.rawBody.hashCode.toString()))
                    .length;
                if (unreadCount == 0) return const SizedBox.shrink();
                return Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 20),
                  child: GestureDetector(
                    onTap: () => _markAllAsRead(
                      notifications.map((m) => m.rawBody.hashCode.toString()).toList(),
                    ),
                    child: Text(
                      'Mark all as read',
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
              child: Consumer<DidcommService>(
                builder: (context, svc, _) {
                  final notifications = svc.messages
                      .where((m) => !m.msgType.contains('payment-auth-request'))
                      .toList();

                  if (notifications.isEmpty) {
                    return Center(
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Icon(LucideIcons.bellOff, size: 40, color: kTextTertiary),
                          const SizedBox(height: 14),
                          Text(
                            'No notifications',
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
                      final msg = notifications[notifications.length - 1 - index];
                      final id = msg.rawBody.hashCode.toString();
                      final isUnread = !_readIds.contains(id);
                      return _NotificationTile(
                        msg: msg,
                        isUnread: isUnread,
                        onTap: () {
                          _markAsRead(id);
                          showDialog(
                            context: context,
                            builder: (ctx) => _NotificationDetailDialog(msg: msg),
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
}

// ---------------------------------------------------------------------------
// Notification Tile
// ---------------------------------------------------------------------------
class _NotificationTile extends StatelessWidget {
  final DecryptedMsg msg;
  final bool isUnread;
  final VoidCallback onTap;

  const _NotificationTile({
    required this.msg,
    required this.isUnread,
    required this.onTap,
  });

  IconData get _icon {
    if (msg.msgType.contains('connection')) return LucideIcons.link;
    if (msg.msgType.contains('list-sync')) return LucideIcons.listChecks;
    return LucideIcons.bell;
  }

  String get _summary {
    if (msg.msgType.contains('connection')) return 'Connection update';
    if (msg.msgType.contains('list-sync')) return msg.listType ?? 'List sync';
    if (msg.description != null && msg.description!.isNotEmpty) return msg.description!;
    return msg.msgType.split('/').last;
  }

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
                color: kCyan.withValues(alpha: 0.08),
                borderRadius: BorderRadius.circular(10),
              ),
              child: Icon(_icon, size: 18, color: kCyan),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    _summary,
                    style: GoogleFonts.inter(
                      fontSize: 13,
                      fontWeight: FontWeight.w600,
                      color: kTextPrimary,
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  const SizedBox(height: 3),
                  Text(
                    msg.msgType.split('/').last,
                    style: GoogleFonts.inter(
                      fontSize: 11,
                      color: kTextTertiary,
                    ),
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
  final DecryptedMsg msg;
  const _NotificationDetailDialog({required this.msg});

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
                  'Notification Detail',
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
            _field('Type', msg.msgType),
            if (msg.listCid != null) _field('List CID', msg.listCid!),
            if (msg.listType != null) _field('List Type', msg.listType!),
            if (msg.label != null) _field('Label', msg.label!),
            if (msg.description != null) _field('Description', msg.description!),
            const SizedBox(height: 12),
            Text('RAW BODY', style: sectionLabel()),
            const SizedBox(height: 6),
            Container(
              width: double.infinity,
              constraints: const BoxConstraints(maxHeight: 200),
              padding: const EdgeInsets.all(10),
              decoration: BoxDecoration(
                color: kSurfaceMid,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: kBorder),
              ),
              child: SingleChildScrollView(
                child: SelectableText(
                  msg.rawBody,
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 10,
                    color: kTextSecondary,
                    height: 1.4,
                  ),
                ),
              ),
            ),
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
