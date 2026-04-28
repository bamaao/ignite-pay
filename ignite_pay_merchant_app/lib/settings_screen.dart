import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/merchant_service.dart';
import 'package:ignite_pay_merchant/services/voice_service.dart';
import 'package:ignite_pay_merchant/services/merchant_push_service.dart';
import 'package:ignite_pay_merchant/notification_center_screen.dart';
import 'package:ignite_pay_merchant/log_viewer_screen.dart';
import 'package:ignite_pay_merchant/profile_screen.dart';
import 'package:ignite_pay_merchant/qr_scanner_screen.dart';
import 'package:ignite_pay_merchant/src/rust/api/merchant.dart' as rust;
import 'package:provider/provider.dart';

class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final svc = context.watch<MerchantService>();
    final voice = context.watch<VoiceService>();
    final pushSvc = context.watch<MerchantPushService>();

    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('设置',
                    style: GoogleFonts.inter(
                      fontSize: 20, fontWeight: FontWeight.w700,
                      color: kTextPrimary, letterSpacing: -0.3,
                    )),
                const SizedBox(height: 20),

                // Merchant Identity
                const SectionLabel(text: '商户身份'),
                const SizedBox(height: 8),
                SettingsTile(
                  icon: LucideIcons.fingerprint,
                  iconColor: kNeonCyan,
                  title: 'DID',
                  subtitle: svc.did.isEmpty ? '未生成' : svc.did,
                  trailing: svc.did.isEmpty
                      ? const SizedBox.shrink()
                      : GestureDetector(
                          onTap: () => Clipboard.setData(ClipboardData(text: svc.did)),
                          child: const Icon(LucideIcons.copy, size: 16, color: kTextSecondary),
                        ),
                ),
                const SizedBox(height: 8),
                FutureBuilder<String>(
                  future: _getPubkey(svc),
                  builder: (_, snap) => SettingsTile(
                    icon: LucideIcons.key,
                    iconColor: kPurple,
                    title: 'Provider Pubkey',
                    subtitle: snap.data ?? '未生成',
                    trailing: snap.data != null
                        ? GestureDetector(
                            onTap: () => Clipboard.setData(ClipboardData(text: snap.data!)),
                            child: const Icon(LucideIcons.copy, size: 16, color: kTextSecondary),
                          )
                        : const SizedBox.shrink(),
                  ),
                ),
                const SizedBox(height: 16),

                // Connection Config
                const SectionLabel(text: '连接配置'),
                const SizedBox(height: 8),
                SettingsTile(
                  icon: LucideIcons.globe,
                  iconColor: kBlue,
                  title: 'Hub Endpoint',
                  subtitle: svc.hubEndpoint.isEmpty ? '未配置' : svc.hubEndpoint,
                  trailing: const Icon(LucideIcons.chevronRight, size: 16, color: kTextSecondary),
                  onTap: () => _editHubEndpoint(context, svc),
                ),
                const SizedBox(height: 8),
                SettingsTile(
                  icon: LucideIcons.radio,
                  iconColor: kCyan,
                  title: 'Mediator WS',
                  subtitle: svc.mediatorWsUrl.isEmpty ? '未配置' : svc.mediatorWsUrl,
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Container(
                        width: 7, height: 7,
                        decoration: BoxDecoration(
                          color: svc.mediatorWsUrl.isNotEmpty ? kSuccess : kDanger,
                          shape: BoxShape.circle,
                        ),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 8),
                SettingsTile(
                  icon: LucideIcons.scanLine,
                  iconColor: kSuccess,
                  title: 'Scan MCP QR Code',
                  subtitle: 'Pair with an MCP agent',
                  trailing: const Icon(LucideIcons.chevronRight, size: 16, color: kTextSecondary),
                  onTap: () => showQrScanner(context),
                ),
                const SizedBox(height: 16),

                // Push Service Status
                const SectionLabel(text: '推送服务'),
                const SizedBox(height: 8),
                SettingsTile(
                  icon: LucideIcons.shield,
                  iconColor: kNeonCyan,
                  title: 'DIDComm DID',
                  subtitle: pushSvc.commDid.isEmpty ? '未初始化' : pushSvc.commDid,
                  trailing: pushSvc.commDid.isEmpty
                      ? const SizedBox.shrink()
                      : GestureDetector(
                          onTap: () => Clipboard.setData(ClipboardData(text: pushSvc.commDid)),
                          child: const Icon(LucideIcons.copy, size: 16, color: kTextSecondary),
                        ),
                ),
                const SizedBox(height: 8),
                SettingsTile(
                  icon: LucideIcons.wifi,
                  iconColor: pushSvc.isConnected ? kSuccess : kDanger,
                  title: 'Mediator 连接',
                  subtitle: pushSvc.isConnected ? '已连接' : '未连接',
                  trailing: Container(
                    width: 7, height: 7,
                    decoration: BoxDecoration(
                      color: pushSvc.isConnected ? kSuccess : kDanger,
                      shape: BoxShape.circle,
                    ),
                  ),
                ),
                const SizedBox(height: 8),
                SettingsTile(
                  icon: LucideIcons.bell,
                  iconColor: kPurple,
                  title: '推送通道',
                  subtitle: pushSvc.pushChannel.isEmpty
                      ? '未配置'
                      : pushSvc.pushChannel == 'websocket'
                          ? 'WebSocket (国内)'
                          : 'FCM (海外)',
                  trailing: const SizedBox.shrink(),
                ),
                const SizedBox(height: 16),

                // Voice
                const SectionLabel(text: '语音播报'),
                const SizedBox(height: 8),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(14),
                  decoration: glassDecoration(),
                  child: Column(
                    children: [
                      Row(
                        children: [
                          Icon(LucideIcons.volume2, size: 17, color: kNeonCyan),
                          const SizedBox(width: 12),
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text('语音播报', style: cardTitle()),
                                Text(voice.enabled ? '已开启' : '已关闭', style: cardSubtitle()),
                              ],
                            ),
                          ),
                          Switch(
                            value: voice.enabled,
                            onChanged: (v) => voice.setEnabled(v),
                            activeThumbColor: kNeonCyan,
                            activeTrackColor: kNeonCyan.withValues(alpha: 0.3),
                          ),
                        ],
                      ),
                      const SizedBox(height: 12),
                      // Language selector
                      Row(
                        children: [
                          Text('语言', style: cardSubtitle()),
                          const SizedBox(width: 12),
                          Expanded(
                            child: SegmentedButton<String>(
                              segments: const [
                                ButtonSegment(value: 'zh-CN', label: Text('中文')),
                                ButtonSegment(value: 'en-US', label: Text('English')),
                              ],
                              selected: {voice.language},
                              onSelectionChanged: (v) => voice.setLanguage(v.first),
                              style: ButtonStyle(
                                visualDensity: VisualDensity.compact,
                                backgroundColor: WidgetStateProperty.resolveWith((states) {
                                  if (states.contains(WidgetState.selected)) {
                                    return kNeonCyan.withValues(alpha: 0.15);
                                  }
                                  return kSurfaceElevated;
                                }),
                                foregroundColor: WidgetStateProperty.resolveWith((states) {
                                  if (states.contains(WidgetState.selected)) return kNeonCyan;
                                  return kTextSecondary;
                                }),
                                side: WidgetStateProperty.all(BorderSide(color: kBorder)),
                                shape: WidgetStateProperty.all(
                                  RoundedRectangleBorder(borderRadius: BorderRadius.circular(6)),
                                ),
                              ),
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 12),
                      // Volume slider
                      Row(
                        children: [
                          Text('音量', style: cardSubtitle()),
                          const SizedBox(width: 12),
                          Expanded(
                            child: Slider(
                              value: voice.volume,
                              onChanged: (v) => voice.setVolume(v),
                              activeColor: kNeonCyan,
                              inactiveColor: kSurfaceElevated,
                            ),
                          ),
                          Text(voice.volume.toStringAsFixed(1),
                              style: monoValue(12)),
                        ],
                      ),
                      const SizedBox(height: 8),
                      // Test button
                      GestureDetector(
                        onTap: () => voice.announcePayment(BigInt.from(100_000_000)),
                        child: Container(
                          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
                          decoration: BoxDecoration(
                            color: kSurfaceElevated,
                            borderRadius: BorderRadius.circular(8),
                            border: Border.all(color: kBorder),
                          ),
                          child: Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              const Icon(LucideIcons.play, size: 14, color: kNeonCyan),
                              const SizedBox(width: 8),
                              Text('测试播报', style: GoogleFonts.inter(fontSize: 13, fontWeight: FontWeight.w600, color: kNeonCyan)),
                            ],
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 16),

                // Quick Access
                const SectionLabel(text: '快捷入口'),
                const SizedBox(height: 8),
                SettingsTile(
                  icon: LucideIcons.user,
                  iconColor: kPurple,
                  title: '商户资料',
                  subtitle: '身份与账户设置',
                  trailing: const Icon(LucideIcons.chevronRight, size: 16, color: kTextSecondary),
                  onTap: () => openProfile(context),
                ),
                const SizedBox(height: 8),
                SettingsTile(
                  icon: LucideIcons.bell,
                  iconColor: kNeonCyan,
                  title: '通知中心',
                  subtitle: '收款与系统通知',
                  trailing: const Icon(LucideIcons.chevronRight, size: 16, color: kTextSecondary),
                  onTap: () => openNotificationCenter(context),
                ),
                const SizedBox(height: 8),
                SettingsTile(
                  icon: LucideIcons.fileText,
                  iconColor: kAmber,
                  title: '查看日志',
                  subtitle: 'DIDComm 与连接调试日志',
                  trailing: const Icon(LucideIcons.chevronRight, size: 16, color: kTextSecondary),
                  onTap: () => openLogViewer(context),
                ),
                const SizedBox(height: 16),

                // About
                const SectionLabel(text: '关于'),
                const SizedBox(height: 8),
                SettingsTile(
                  icon: LucideIcons.info,
                  iconColor: kTextSecondary,
                  title: '版本',
                  subtitle: '1.0.0',
                  trailing: const SizedBox.shrink(),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Future<String> _getPubkey(MerchantService svc) async {
    if (svc.hubEndpoint.isEmpty) return '未配置';
    try {
      return await rust.getMerchantPubkey(
        storagePath: svc.storagePath,
      );
    } catch (_) {
      return '未生成';
    }
  }

  void _editHubEndpoint(BuildContext context, MerchantService svc) {
    final controller = TextEditingController(text: svc.hubEndpoint);
    showDialog(
      context: context,
      builder: (_) => AlertDialog(
        backgroundColor: kSurfaceDark,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
        title: Text('Hub Endpoint', style: GoogleFonts.inter(color: kTextPrimary)),
        content: TextField(
          controller: controller,
          style: GoogleFonts.jetBrainsMono(fontSize: 14, color: kTextPrimary),
          decoration: InputDecoration(
            hintText: 'https://hub.example.com',
            hintStyle: GoogleFonts.jetBrainsMono(fontSize: 14, color: kTextTertiary),
            filled: true,
            fillColor: kSurfaceElevated,
            border: OutlineInputBorder(borderRadius: BorderRadius.circular(8), borderSide: const BorderSide(color: kBorder)),
            enabledBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(8), borderSide: const BorderSide(color: kBorder)),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: Text('取消', style: GoogleFonts.inter(color: kTextSecondary)),
          ),
          TextButton(
            onPressed: () {
              svc.saveConfig(controller.text, svc.mediatorWsUrl);
              Navigator.pop(context);
            },
            child: Text('保存', style: GoogleFonts.inter(color: kNeonCyan)),
          ),
        ],
      ),
    );
  }
}
