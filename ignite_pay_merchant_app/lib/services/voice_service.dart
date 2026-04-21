import 'package:flutter/material.dart';
import 'package:flutter_tts/flutter_tts.dart';
import 'package:shared_preferences/shared_preferences.dart';

class VoiceService extends ChangeNotifier {
  final FlutterTts _tts = FlutterTts();

  bool _enabled = true;
  String _language = 'zh-CN';
  double _volume = 0.8;

  bool get enabled => _enabled;
  String get language => _language;
  double get volume => _volume;

  Future<void> initialize() async {
    final prefs = await SharedPreferences.getInstance();
    _enabled = prefs.getBool('voice_enabled') ?? true;
    _language = prefs.getString('voice_language') ?? 'zh-CN';
    _volume = prefs.getDouble('voice_volume') ?? 0.8;

    await _tts.setLanguage(_language);
    await _tts.setVolume(_volume);
    await _tts.setSpeechRate(0.5);
  }

  Future<void> announcePayment(BigInt amount) async {
    if (!_enabled) return;
    final display = (amount.toDouble() / 1_000_000_000).toStringAsFixed(2);
    final msg = _language == 'zh-CN'
        ? '收到收款 $display USDC'
        : 'Payment received: $display USDC';
    await _tts.speak(msg);
  }

  Future<void> setEnabled(bool value) async {
    _enabled = value;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool('voice_enabled', value);
    notifyListeners();
  }

  Future<void> setLanguage(String lang) async {
    _language = lang;
    await _tts.setLanguage(lang);
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('voice_language', lang);
    notifyListeners();
  }

  Future<void> setVolume(double v) async {
    _volume = v;
    await _tts.setVolume(v);
    final prefs = await SharedPreferences.getInstance();
    await prefs.setDouble('voice_volume', v);
    notifyListeners();
  }
}
