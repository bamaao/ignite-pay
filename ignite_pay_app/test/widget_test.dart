import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ignite_pay_app/main.dart';

void main() {
  testWidgets('App launches smoke test', (WidgetTester tester) async {
    await tester.pumpWidget(const MaterialApp(home: IgnitePayDashboard()));
    expect(find.textContaining('Ignite Pay'), findsWidgets);
  });
}
