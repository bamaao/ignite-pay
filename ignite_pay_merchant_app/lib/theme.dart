// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';

// ---------------------------------------------------------------------------
// Shared Theme Constants — Dark Glassmorphism (Ignite Pay Merchant)
// ---------------------------------------------------------------------------

// Backgrounds
const kBackground = Color(0xFF0A0A14);
const kSurfaceDark = Color(0xFF12121F);
const kSurfaceMid = Color(0xFF1A1A2E);
const kSurfaceElevated = Color(0xFF22223A);

// Borders
const kBorder = Color(0xFF22223A);
const kGlassBorder = Color(0x1AFFFFFF);

// Text
const kTextPrimary = Color(0xFFF0F0F8);
const kTextSecondary = Color(0xFF7A7A96);
const kTextTertiary = Color(0xFF4A4A64);

// Accent colors — Orange-Red theme
const kNeonCyan = Color(0xFFFF5722);
const kNeonCyanDim = Color(0xFFBF360C);
const kPurple = Color(0xFFFF8A50);
const kPurpleDim = Color(0xFFE64A19);
const kBlue = Color(0xFFFF9100);
const kCyan = Color(0xFFFF6E40);

// Status colors
const kSuccess = Color(0xFF00E676);
const kPending = Color(0xFFFFB300);
const kAmber = Color(0xFFFFB300);
const kDanger = Color(0xFFFF5252);
const kIntercepted = Color(0xFFFF5252);

// ---------------------------------------------------------------------------
// Reusable Text Styles
// ---------------------------------------------------------------------------

TextStyle sectionLabel() => GoogleFonts.inter(
      fontSize: 10,
      fontWeight: FontWeight.w700,
      color: kTextTertiary,
      letterSpacing: 1.5,
    );

TextStyle cardTitle() => GoogleFonts.inter(
      fontSize: 14,
      fontWeight: FontWeight.w600,
      color: kTextPrimary,
    );

TextStyle cardSubtitle() => GoogleFonts.inter(
      fontSize: 11,
      color: kTextSecondary,
    );

TextStyle monoValue([double fontSize = 14]) => GoogleFonts.jetBrainsMono(
      fontSize: fontSize,
      fontWeight: FontWeight.w500,
      color: kTextPrimary,
    );

// ---------------------------------------------------------------------------
// Reusable Decorations
// ---------------------------------------------------------------------------

BoxDecoration glassDecoration({Color? accentBorder}) => BoxDecoration(
      color: kSurfaceDark,
      borderRadius: BorderRadius.circular(12),
      border: Border.all(color: accentBorder ?? kBorder),
    );

BoxDecoration glassCardDecoration() => BoxDecoration(
      color: kSurfaceMid.withValues(alpha: 0.6),
      borderRadius: BorderRadius.circular(16),
      border: Border.all(color: kGlassBorder),
      gradient: LinearGradient(
        colors: [
          kSurfaceMid.withValues(alpha: 0.7),
          kSurfaceDark.withValues(alpha: 0.5),
        ],
        begin: Alignment.topLeft,
        end: Alignment.bottomRight,
      ),
    );

// ---------------------------------------------------------------------------
// Reusable Widgets
// ---------------------------------------------------------------------------

class BackButtonGlass extends StatelessWidget {
  final VoidCallback? onTap;
  const BackButtonGlass({super.key, this.onTap});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap ?? () => Navigator.of(context).pop(),
      child: Container(
        width: 36,
        height: 36,
        decoration: BoxDecoration(
          color: kSurfaceDark,
          borderRadius: BorderRadius.circular(8),
          border: Border.all(color: kBorder),
        ),
        child: const Icon(Icons.arrow_back, size: 18, color: kTextSecondary),
      ),
    );
  }
}

class PageHeader extends StatelessWidget {
  final String title;
  final String? subtitle;
  final VoidCallback? onBack;

  const PageHeader({super.key, required this.title, this.subtitle, this.onBack});

  @override
  Widget build(BuildContext context) {
    final canPop = Navigator.of(context).canPop();
    return Row(
      children: [
        if (canPop) ...[
          BackButtonGlass(onTap: onBack),
          const SizedBox(width: 14),
        ],
        Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(title, style: GoogleFonts.inter(
              fontSize: 20, fontWeight: FontWeight.w700,
              color: kTextPrimary, letterSpacing: -0.3,
            )),
            if (subtitle != null)
              Text(subtitle!, style: cardSubtitle()),
          ],
        ),
      ],
    );
  }
}

class SettingsTile extends StatelessWidget {
  final IconData icon;
  final Color iconColor;
  final String title;
  final String? subtitle;
  final Widget trailing;
  final VoidCallback? onTap;
  final Color? accentBorder;

  const SettingsTile({
    super.key,
    required this.icon,
    required this.iconColor,
    required this.title,
    this.subtitle,
    required this.trailing,
    this.onTap,
    this.accentBorder,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.all(14),
        decoration: glassDecoration(accentBorder: accentBorder),
        child: Row(
          children: [
            Container(
              width: 36,
              height: 36,
              decoration: BoxDecoration(
                color: iconColor.withValues(alpha: 0.08),
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: iconColor.withValues(alpha: 0.15)),
              ),
              child: Icon(icon, size: 17, color: iconColor),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(title, style: cardTitle()),
                  if (subtitle != null) ...[
                    const SizedBox(height: 2),
                    Text(subtitle!, style: cardSubtitle()),
                  ],
                ],
              ),
            ),
            trailing,
          ],
        ),
      ),
    );
  }
}

class SectionLabel extends StatelessWidget {
  final String text;
  const SectionLabel({super.key, required this.text});

  @override
  Widget build(BuildContext context) {
    return Text(text, style: sectionLabel());
  }
}
