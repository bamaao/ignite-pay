import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ignite_pay_app/main.dart';
import 'package:ignite_pay_app/src/rust/frb_generated.dart';
import 'package:integration_test/integration_test.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async => await RustLib.init());
  testWidgets('App launches', (WidgetTester tester) async {
    await tester.pumpWidget(const MaterialApp(home: IgnitePayDashboard()));
    expect(find.textContaining('Ignite Pay'), findsWidgets);
  });
}
