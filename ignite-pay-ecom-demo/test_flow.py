"""
End-to-end test script for the Ignite Demo Store x402 payment flow.

Two modes:
  python test_flow.py              — Standalone mock flow (no MCP/phone needed)
  python test_flow.py --instructions — Print step-by-step real flow instructions
"""

import base64
import json
import sys
from pathlib import Path

import httpx

BASE_URL = "http://localhost:9090"
CONFIG_PATH = Path(__file__).parent / "config.json"

with open(CONFIG_PATH) as f:
    CONFIG = json.load(f)


def print_instructions():
    """Print step-by-step instructions for the real end-to-end flow."""
    print("=" * 72)
    print("  Ignite Pay — End-to-End x402 Payment Flow Instructions")
    print("=" * 72)
    print()
    print("Prerequisites:")
    print("  1. All backend services running:  .\\deploy-local.ps1 start")
    print("  2. config.json updated with merchant DID and payment address")
    print("     (get them from the merchant MCP's get_identity tool)")
    print("  3. Phone app paired with ignite-pay-mcp via QR code")
    print("  4. E-commerce server running:  cd ignite-pay-ecom-demo && python server.py")
    print()
    print("Steps:")
    print("-" * 72)
    print()
    print("  1. AI Agent discovers products:")
    print("     GET http://localhost:9090/products")
    print()
    print("  2. AI Agent places an order:")
    print('     POST http://localhost:9090/orders  {"product_id": "coffee"}')
    print("     -> Server returns HTTP 402 with Coinbase x402 PaymentRequirements")
    print()
    print("  3. AI Agent calls ignite-pay-mcp tool:")
    print("     process_x402_challenge(challenge_body, headers)")
    print("     The MCP parses the 402 response and creates a PaymentRequest")
    print()
    print("  4. MCP sends payment-auth-request to phone via DIDComm:")
    print("     MCP -> didcomm-router -> Phone App")
    print()
    print("  5. Phone App shows payment request, user approves")
    print()
    print("  6. Phone App creates session key, registers on-chain")
    print("     (via local wallet signing on Solana devnet)")
    print()
    print("  7. Fund the session key (devnet airdrop):")
    print("     python fund_session.py <session_pubkey_base58>")
    print()
    print("  8. Phone App sends payment-auth-response back via DIDComm:")
    print("     Phone App -> didcomm-router -> MCP")
    print()
    print("  9. MCP executes SOL transfer via session key on Solana devnet")
    print()
    print(" 10. AI Agent retries order with payment proof:")
    print('     POST http://localhost:9090/orders  {"product_id": "coffee"}')
    print("     Header: X-Payment-Proof: <tx_signature>")
    print("     -> Server verifies on-chain, returns order confirmation")
    print()
    print(" 11. Verify order status:")
    print("     GET http://localhost:9090/orders/{order_id}")
    print()
    print("-" * 72)
    print()
    print("Expected Coinbase x402 PaymentRequirements Format:")
    print(json.dumps({
        "scheme": "exact",
        "network": CONFIG["solana"]["network"],
        "maxTimeoutSeconds": CONFIG["x402"]["maxTimeoutSeconds"],
        "amount": "100000",
        "asset": CONFIG["solana"]["asset_native"],
        "payTo": CONFIG["merchant"]["payment_address"],
        "extra": {
            "memo": CONFIG["merchant"]["did"],
        },
    }, indent=2))
    print()


def run_mock_flow():
    """Run standalone mock flow against the e-commerce server."""
    print("=" * 72)
    print("  Ignite Demo Store — Standalone Mock Flow Test")
    print("=" * 72)
    print()

    client = httpx.Client(base_url=BASE_URL, timeout=10.0)

    # Step 1: Health check
    print("[1] GET /health")
    resp = client.get("/health")
    assert resp.status_code == 200, f"Health check failed: {resp.status_code}"
    health = resp.json()
    print(f"    Status: {health['status']}")
    print(f"    Merchant DID: {health['merchant_did']}")
    print()

    # Step 2: List products
    print("[2] GET /products")
    resp = client.get("/products")
    assert resp.status_code == 200, f"Products failed: {resp.status_code}"
    products = resp.json()["products"]
    for p in products:
        print(f"    {p['id']}: {p['name']} — {p['price_lamports']} lamports ({p['price_sol']} SOL)")
    print()

    # Step 3: Create order (expect 402 with Coinbase x402 format)
    print("[3] POST /orders  {product_id: coffee}")
    resp = client.post("/orders", json={"product_id": "coffee"})
    assert resp.status_code == 402, f"Expected 402, got {resp.status_code}"
    challenge = resp.json()
    order_id = resp.headers.get("x402-order-id")
    print(f"    Status: 402 Payment Required")
    print(f"    Order ID: {order_id}")

    # Validate Coinbase x402 PaymentRequirements fields
    print(f"    PaymentRequirements:")
    print(f"      scheme:    {challenge['scheme']}")
    print(f"      network:   {challenge['network']}")
    print(f"      amount:    {challenge['amount']}")
    print(f"      asset:     {challenge['asset']}")
    print(f"      payTo:     {challenge['payTo']}")
    print(f"      maxTimeoutSeconds: {challenge['maxTimeoutSeconds']}")

    assert challenge["scheme"] == "exact"
    assert challenge["network"] == CONFIG["solana"]["network"]
    assert challenge["amount"] == "100000"
    assert challenge["asset"] == CONFIG["solana"]["asset_native"]
    assert challenge["payTo"] == CONFIG["merchant"]["payment_address"]
    assert challenge["maxTimeoutSeconds"] == CONFIG["x402"]["maxTimeoutSeconds"]
    assert challenge["extra"]["memo"] == CONFIG["merchant"]["did"]

    # Validate PAYMENT-REQUIRED header (base64-encoded PaymentRequirements)
    payment_required_header = resp.headers.get("payment-required")
    assert payment_required_header is not None, "Missing PAYMENT-REQUIRED header"
    decoded = json.loads(base64.b64decode(payment_required_header))
    assert decoded["scheme"] == "exact"
    assert decoded["network"] == CONFIG["solana"]["network"]
    assert decoded["amount"] == "100000"
    assert decoded["payTo"] == CONFIG["merchant"]["payment_address"]
    print(f"    PAYMENT-REQUIRED header: VALID (base64-encoded PaymentRequirements)")
    print()

    # Step 4: Check order status (should be pending_payment)
    print(f"[4] GET /orders/{order_id}")
    resp = client.get(f"/orders/{order_id}")
    assert resp.status_code == 200
    order = resp.json()
    print(f"    Status: {order['status']}")
    print(f"    Product: {order['product_name']}")
    assert order["status"] == "pending_payment"
    print()

    # Step 5: Verify with mock tx signature (will fail verification — expected on devnet)
    print(f"[5] POST /orders/{order_id}/verify-tx  (mock tx — expect verification failure)")
    resp = client.post(f"/orders/{order_id}/verify-tx", json={
        "tx_signature": "MOCK_TX_SIGNATURE_FOR_TESTING_ONLY_THIS_IS_NOT_REAL"
    })
    assert resp.status_code == 400, f"Expected 400 for invalid tx, got {resp.status_code}"
    print(f"    Status: 400 (expected — mock signature is not on-chain)")
    print()

    # Step 6: Create another order with a fake X-Payment-Proof (also fails)
    print("[6] POST /orders with X-Payment-Proof header (mock — expect failure)")
    resp = client.post(
        "/orders",
        json={"product_id": "sandwich"},
        headers={"X-Payment-Proof": "MOCK_TX_NOT_REAL"},
    )
    assert resp.status_code == 400
    print(f"    Status: 400 (expected — mock signature is not on-chain)")
    print()

    # Step 7: Test invalid product
    print("[7] POST /orders  {product_id: invalid}")
    resp = client.post("/orders", json={"product_id": "invalid"})
    assert resp.status_code == 400
    print(f"    Status: 400 (expected — unknown product)")
    print()

    # Step 8: Test order not found
    print("[8] GET /orders/nonexistent")
    resp = client.get("/orders/nonexistent123")
    assert resp.status_code == 404
    print(f"    Status: 404 (expected — order not found)")
    print()

    print("=" * 72)
    print("  All mock flow tests passed!")
    print("=" * 72)
    print()
    print("To test the real payment flow:")
    print("  1. Configure config.json with a real merchant DID and payment address")
    print("  2. Run: python test_flow.py --instructions")


def main():
    if "--instructions" in sys.argv:
        print_instructions()
    else:
        run_mock_flow()


if __name__ == "__main__":
    main()
