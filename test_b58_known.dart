import 'dart:convert';
import 'dart:typed_data';

const _b58Alphabet =
    '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

Uint8List _b58Decode(String input) {
  int zeros = 0;
  while (zeros < input.length && input[zeros] == '1') {
    zeros++;
  }
  List<int> bytes = [];
  for (int i = zeros; i < input.length; i++) {
    int carry = _b58Alphabet.indexOf(input[i]);
    if (carry < 0) {
      throw FormatException('Invalid base58 character: ${input[i]}');
    }
    for (int j = 0; j < bytes.length; j++) {
      carry += bytes[j] * 58;
      bytes[j] = carry & 0xFF;
      carry >>= 8;
    }
    while (carry > 0) {
      bytes.add(carry & 0xFF);
      carry >>= 8;
    }
  }
  final result = BytesBuilder();
  for (int i = 0; i < zeros; i++) {
    result.addByte(0);
  }
  for (int i = bytes.length - 1; i >= 0; i--) {
    result.addByte(bytes[i]);
  }
  return result.toBytes();
}

String _b58Encode(List<int> input) {
  final bytes = Uint8List.fromList(input);
  int zeros = 0;
  while (zeros < bytes.length && bytes[zeros] == 0) {
    zeros++;
  }
  List<int> encoded = [];
  for (int i = zeros; i < bytes.length; i++) {
    int carry = bytes[i];
    for (int j = 0; j < encoded.length; j++) {
      carry += encoded[j] * 256;
      encoded[j] = carry % 58;
      carry ~/= 58;
    }
    while (carry > 0) {
      encoded.add(carry % 58);
      carry ~/= 58;
    }
  }
  final sb = StringBuffer();
  for (int i = 0; i < zeros; i++) {
    sb.write('1');
  }
  for (int i = encoded.length - 1; i >= 0; i--) {
    sb.write(_b58Alphabet[encoded[i]]);
  }
  return sb.toString();
}

void main() {
  // Test: "Hello World!" base58 = 2NEpo7TZRRrLgSi8PjwXjY (but actually let's verify encode first)
  final helloBytes = utf8.encode('Hello World!');
  final helloB58 = _b58Encode(helloBytes);
  final helloRoundtrip = _b58Decode(helloB58);
  print('Hello World! -> b58: $helloB58');
  print('roundtrip: ${utf8.decode(helloRoundtrip)}');

  // Solana system program: 32 zero bytes
  final zeros32 = List.filled(32, 0);
  final zerosB58 = _b58Encode(zeros32);
  print('32 zeros -> b58: $zerosB58 (expect 11111111111111111111111111111111)');

  // A real Solana pubkey roundtrip
  final realPubkey = '9aE476fH7wwJyCv3pCmLa9rX2cFRkpLWg5tW7uHf2spt';
  final decoded = _b58Decode(realPubkey);
  print('Real pubkey decoded: ${decoded.length} bytes (expect 32)');
  final reencoded = _b58Encode(decoded);
  print('Re-encoded: $reencoded');
  print('Match: ${reencoded == realPubkey}');

  // b64 -> b58 -> b64 roundtrip (simulating Phantom key exchange)
  final testKey = Uint8List.fromList(List.generate(32, (i) => i + 1));
  final b64 = base64Url.encode(testKey).replaceAll('=', '');
  // b64 decode -> b58 encode -> b58 decode -> b64 encode
  final b64decoded = base64Url.decode(b64 + '=' * (4 - b64.length % 4));
  final b58 = _b58Encode(b64decoded);
  final b58decoded = _b58Decode(b58);
  final b64rt = base64Url.encode(b58decoded).replaceAll('=', '');
  print('b64->b58->b64 roundtrip match: ${b64 == b64rt}');
}
