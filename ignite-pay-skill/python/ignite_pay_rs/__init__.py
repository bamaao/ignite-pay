# Copyright (c) 2026 zouyc zouyccq@gmail.com.
# All rights reserved.
#
# Licensed under the Business Source License 1.1 (BSL 1.1).
# You may not use this file except in compliance with the License.
#
# Change Date: 2031-01-01
# On the Change Date, or the fourth anniversary of the first publicly available
# distribution of the code under the BSL, whichever comes first, the code
# automatically becomes available under the Apache License 2.0.

"""Ignite Pay Rust extension - Python wrapper for the Ignite Pay DIDComm skill.

Exposes IgnitePayCore with the following methods:
    - new() -> IgnitePayCore
    - init_list_store(db_path: str) -> None
    - start_listener(ws_url: str) -> None
    - check_and_pay(merchant_did: str, amount: int) -> str  (async)
    - check_allowance(merchant_did: str, amount: int = None) -> str
    - risk_check(merchant_did: str, amount: int) -> str
    - add_to_whitelist(did, name=None, max_amount=None, label=None) -> None
    - remove_from_whitelist(did) -> None
    - add_to_blacklist(did, name=None) -> None
    - remove_from_blacklist(did) -> None
    - our_did (property) -> str

Also exposes:
    - process_x402(mcp_url, challenge_body, **kwargs) -> dict
"""

from ignite_pay_rs.ignite_pay_rs import IgnitePayCore

__all__ = ["IgnitePayCore", "process_x402"]


def process_x402(
    mcp_url: str,
    challenge_body: str,
    *,
    phone_did: str = "",
    x402_merchant_did: str = None,
    x402_payment_address: str = None,
    x402_merkle_context: str = None,
    vc_ipfs_cid: str = None,
    timeout: float = 310.0,
) -> dict:
    """Call the MCP REST API to process an x402 payment challenge.

    Args:
        mcp_url: Base URL of the MCP server (e.g. "http://127.0.0.1:9001").
        challenge_body: The HTTP 402 response body as a JSON string.
        phone_did: Optional phone DID for authorization.
        x402_merchant_did: Optional merchant DID header override.
        x402_payment_address: Optional payment address header override.
        x402_merkle_context: Optional merkle context header.
        vc_ipfs_cid: Optional IPFS CID for a Verifiable Credential.
        timeout: Request timeout in seconds (default 310s, covers MCP's 300s auth timeout).

    Returns:
        dict with keys:
            - status: "success" | "rejected" | "error"
            - On success: payment_id, proof, amount, token, recipient, merchant_did, method
            - On rejected: payment_id, reason
            - On error: payment_id (optional), message

    Raises:
        httpx.HTTPError: On network/connection errors.
    """
    import httpx

    body = {
        "challenge_body": challenge_body,
        "phone_did": phone_did,
    }
    if x402_merchant_did is not None:
        body["x402_merchant_did"] = x402_merchant_did
    if x402_payment_address is not None:
        body["x402_payment_address"] = x402_payment_address
    if x402_merkle_context is not None:
        body["x402_merkle_context"] = x402_merkle_context
    if vc_ipfs_cid is not None:
        body["vc_ipfs_cid"] = vc_ipfs_cid

    url = f"{mcp_url.rstrip('/')}/api/x402"

    with httpx.Client(timeout=timeout) as client:
        response = client.post(url, json=body)

    result = response.json()
    result["http_status"] = response.status_code
    return result
