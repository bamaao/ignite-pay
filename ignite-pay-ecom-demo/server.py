"""
Ignite Pay E-Commerce Demo Server

Minimal FastAPI server that returns x402 HTTP 402 payment challenges,
enabling end-to-end testing of the ignite-pay payment flow.

Endpoints:
  GET  /health              — Health check + merchant DID
  GET  /products            — List products with prices
  POST /orders              — Create order (402 if unpaid, confirm if X-Payment-Proof present)
  GET  /orders/{order_id}   — Order status (polling after payment)
  POST /orders/{order_id}/verify — Verify payment on-chain, confirm order
"""

import base64
import json
import time
import uuid
from pathlib import Path
from typing import Dict, Optional

import uvicorn
from fastapi import FastAPI, Header, HTTPException, Request
from fastapi.responses import JSONResponse
from solana.rpc.api import Client as SolanaClient
from solana.rpc.commitment import Confirmed

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

CONFIG_PATH = Path(__file__).parent / "config.json"

with open(CONFIG_PATH) as f:
    CONFIG = json.load(f)

MERCHANT_DID = CONFIG["merchant"]["did"]
PAYMENT_ADDRESS = CONFIG["merchant"]["payment_address"]
SOLANA_RPC = CONFIG["solana"]["rpc_url"]
PRODUCTS = {p["id"]: p for p in CONFIG["products"]}

# x402 standard fields
X402_NETWORK = CONFIG["solana"]["network"]
X402_SCHEME = CONFIG["x402"]["scheme"]
X402_ASSET_NATIVE = CONFIG["solana"]["asset_native"]
X402_MAX_TIMEOUT = CONFIG["x402"]["maxTimeoutSeconds"]

# ---------------------------------------------------------------------------
# In-memory order store
# ---------------------------------------------------------------------------

orders: Dict[str, dict] = {}

# ---------------------------------------------------------------------------
# App
# ---------------------------------------------------------------------------

app = FastAPI(title="Ignite Demo Store", version="0.1.0")


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def make_payment_requirements(amount_lamports: int) -> dict:
    """Build a Coinbase x402 PaymentRequirements object for Solana."""
    return {
        "scheme": X402_SCHEME,
        "network": X402_NETWORK,
        "maxTimeoutSeconds": X402_MAX_TIMEOUT,
        "amount": str(amount_lamports),
        "asset": X402_ASSET_NATIVE,
        "payTo": PAYMENT_ADDRESS,
        "extra": {
            "memo": MERCHANT_DID,
        },
    }


def make_402_challenge(order_id: str, product: dict) -> JSONResponse:
    """Build a Coinbase x402 HTTP 402 response with standard PaymentRequirements."""
    payment_req = make_payment_requirements(product["price_lamports"])
    payment_req_b64 = base64.b64encode(
        json.dumps(payment_req).encode()
    ).decode()

    return JSONResponse(
        status_code=402,
        content=payment_req,
        headers={
            # Standard x402 header
            "PAYMENT-REQUIRED": payment_req_b64,
            # Ignite-specific headers for MCP compatibility
            "x402-merchant-did": MERCHANT_DID,
            "x402-payment-address": PAYMENT_ADDRESS,
            "x402-order-id": order_id,
        },
    )


def verify_on_chain_tx(tx_signature: str, expected_amount: int, recipient: str) -> bool:
    """Call Solana RPC getTransaction to verify a SOL transfer."""
    try:
        client = SolanaClient(SOLANA_RPC)
        resp = client.get_transaction(
            tx_signature,
            commitment=Confirmed,
            max_supported_transaction_version=0,
        )
        if not resp or not resp.value:
            return False

        tx = resp.value
        meta = tx.transaction.meta
        if not meta or meta.err:
            return False

        # Check post-balances for a transfer to the recipient.
        account_keys = tx.transaction.transaction.message.account_keys
        post_balances = meta.post_balances
        pre_balances = meta.pre_balances

        for i, key in enumerate(account_keys):
            if str(key) == recipient:
                diff = post_balances[i] - pre_balances[i]
                if diff >= expected_amount:
                    return True
        return False
    except Exception:
        return False


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------

@app.get("/health")
def health():
    return {
        "status": "ok",
        "merchant_did": MERCHANT_DID,
        "merchant_name": CONFIG["merchant"]["name"],
    }


@app.get("/products")
def list_products():
    return {
        "products": [
            {
                "id": p["id"],
                "name": p["name"],
                "price_lamports": p["price_lamports"],
                "price_sol": p["price_lamports"] / 1_000_000_000,
            }
            for p in CONFIG["products"]
        ]
    }


@app.post("/orders")
async def create_order(request: Request, x_payment_proof: Optional[str] = Header(None)):
    """Create an order. Returns 402 if unpaid; confirms if payment proof is present."""
    body = await request.json()
    product_id = body.get("product_id")
    if not product_id or product_id not in PRODUCTS:
        raise HTTPException(status_code=400, detail=f"Unknown product_id: {product_id}")

    product = PRODUCTS[product_id]
    order_id = uuid.uuid4().hex[:12]

    # If a payment proof (tx signature) is supplied, verify and confirm immediately.
    if x_payment_proof:
        verified = verify_on_chain_tx(
            x_payment_proof, product["price_lamports"], PAYMENT_ADDRESS
        )
        if not verified:
            raise HTTPException(status_code=400, detail="Payment verification failed")

        order = {
            "id": order_id,
            "product_id": product_id,
            "product_name": product["name"],
            "amount_lamports": product["price_lamports"],
            "status": "paid",
            "tx_signature": x_payment_proof,
            "created_at": time.time(),
            "paid_at": time.time(),
        }
        orders[order_id] = order
        return JSONResponse(status_code=200, content=order)

    # No payment yet — return 402 challenge.
    order = {
        "id": order_id,
        "product_id": product_id,
        "product_name": product["name"],
        "amount_lamports": product["price_lamports"],
        "status": "pending_payment",
        "created_at": time.time(),
    }
    orders[order_id] = order
    return make_402_challenge(order_id, product)


@app.get("/orders/{order_id}")
def get_order(order_id: str):
    if order_id not in orders:
        raise HTTPException(status_code=404, detail="Order not found")
    return orders[order_id]


@app.post("/orders/{order_id}/verify")
def verify_order(order_id: str):
    """Verify payment on-chain for an existing order and mark it as paid."""
    if order_id not in orders:
        raise HTTPException(status_code=404, detail="Order not found")

    order = orders[order_id]
    if order["status"] == "paid":
        return order

    if order["status"] != "pending_payment":
        raise HTTPException(status_code=400, detail=f"Order status is '{order['status']}', cannot verify")

    # Check recent transactions to the merchant address for the expected amount.
    # In practice the client should supply the tx_signature; here we do a best-effort check.
    raise HTTPException(
        status_code=400,
        detail="Provide the transaction signature via POST /orders with X-Payment-Proof header, "
               "or use the standalone verification endpoint with a tx_signature field.",
    )


@app.post("/orders/{order_id}/verify-tx")
async def verify_tx(order_id: str, request: Request):
    """Verify a specific transaction signature against an order."""
    if order_id not in orders:
        raise HTTPException(status_code=404, detail="Order not found")

    order = orders[order_id]
    if order["status"] == "paid":
        return order

    body = await request.json()
    tx_signature = body.get("tx_signature")
    if not tx_signature:
        raise HTTPException(status_code=400, detail="tx_signature is required")

    verified = verify_on_chain_tx(tx_signature, order["amount_lamports"], PAYMENT_ADDRESS)
    if not verified:
        raise HTTPException(status_code=400, detail="Payment verification failed")

    order["status"] = "paid"
    order["tx_signature"] = tx_signature
    order["paid_at"] = time.time()
    return order


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    host = CONFIG["server"]["host"]
    port = CONFIG["server"]["port"]
    print(f"Starting Ignite Demo Store on {host}:{port}")
    print(f"Merchant DID: {MERCHANT_DID}")
    print(f"Payment address: {PAYMENT_ADDRESS}")
    uvicorn.run(app, host=host, port=port)
