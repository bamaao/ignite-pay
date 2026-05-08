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

"""Ignite Pay Skill - Python SDK for agent-initiated payments."""
import json
from ignite_pay_rs import IgnitePayCore, process_x402 as _mcp_process_x402


class IgnitePaySkill:
    """High-level SDK for agent-initiated payments via Ignite Pay.

    Supports two modes:
    1. MCP API mode (recommended): delegates all payment logic to ignite-pay-mcp
       via REST API. Initialize with `mcp_url`.
    2. Local mode (legacy): uses the Rust core directly for whitelist/blacklist
       and risk checks. Initialize with `mediator_url`.

    Usage (MCP API mode):
        skill = IgnitePaySkill(mcp_url="http://127.0.0.1:9001")
        result = skill.process_x402(challenge_body, headers)

    Usage (local mode):
        skill = IgnitePaySkill(mediator_url="wss://mediator.ignite.com")
        result = await skill.pay_merchant("did:ignite:z...", 10.0)
    """

    def __init__(self, mediator_url: str = None, db_path: str = None, mcp_url: str = None):
        """Initialize the payment skill.

        Args:
            mediator_url: Mediator WebSocket URL for local mode (legacy).
            db_path: Whitelist/blacklist persistence path (optional, for local mode).
            mcp_url: MCP server URL for API mode (e.g. "http://127.0.0.1:9001").
        """
        self.mcp_url = mcp_url
        self.mediator_url = mediator_url

        if mediator_url:
            self.core = IgnitePayCore()
            if db_path:
                self.core.init_identity(db_path)
                self.core.init_list_store(db_path)
            self.core.start_listener(mediator_url)
        else:
            self.core = None

    def process_x402(
        self,
        challenge_body: str,
        *,
        phone_did: str = "",
        x402_merchant_did: str = None,
        x402_payment_address: str = None,
        x402_merkle_context: str = None,
        vc_ipfs_cid: str = None,
    ) -> dict:
        """Process an HTTP 402 payment challenge via the MCP REST API.

        This is the primary entry point for agent-initiated payments.
        The MCP server handles: x402 parsing, merchant verification,
        risk control, DIDComm phone authorization, and on-chain payment.

        Args:
            challenge_body: The HTTP 402 response body (JSON string).
            phone_did: Optional phone DID for authorization.
            x402_merchant_did: Optional merchant DID from x402-merchant-did header.
            x402_payment_address: Optional payment address from x402-payment-address header.
            x402_merkle_context: Optional merkle context from x402-merkle-context header.
            vc_ipfs_cid: Optional IPFS CID for a Verifiable Credential.

        Returns:
            dict with status "success", "rejected", or "error".
            On success: includes payment_id, proof, amount, token, recipient, merchant_did.
            On rejected: includes payment_id, reason.
            On error: includes payment_id (optional), message.

        Raises:
            ValueError: If mcp_url is not configured.
            httpx.HTTPError: On network errors.
        """
        if not self.mcp_url:
            raise ValueError("mcp_url must be configured to use process_x402(). "
                             "Initialize with IgnitePaySkill(mcp_url='http://...')")

        return _mcp_process_x402(
            self.mcp_url,
            challenge_body,
            phone_did=phone_did,
            x402_merchant_did=x402_merchant_did,
            x402_payment_address=x402_payment_address,
            x402_merkle_context=x402_merkle_context,
            vc_ipfs_cid=vc_ipfs_cid,
        )

    async def pay_merchant(self, merchant_did: str, amount: float, reason: str = "Payment") -> dict:
        """Execute a payment via local Rust core (legacy).

        .. deprecated::
            Use `process_x402()` with MCP API mode instead.

        Args:
            merchant_did: The merchant's DID identifier.
            amount: Payment amount (natural units, will be converted to lamports).
            reason: Human-readable payment description.

        Returns:
            dict with 'success' and 'tx_sig' or 'error'.
        """
        if not self.core:
            return {"success": False, "error": "Local mode requires mediator_url"}
        lamports = int(amount * 10**9)
        try:
            tx_sig = await self.core.check_and_pay(merchant_did, lamports)
            return {"success": True, "tx_sig": tx_sig}
        except Exception as e:
            return {"success": False, "error": str(e)}

    def check_allowance(self, merchant_did: str, amount: float = None) -> dict:
        """Query merchant allowance from the local whitelist/blacklist.

        Args:
            merchant_did: The merchant's DID to check.
            amount: Optional amount (in natural units) for whitelist limit checking.

        Returns:
            dict with is_blacklisted, is_whitelisted, max_amount, label, expires_at.
        """
        if not self.core:
            raise ValueError("Local mode requires mediator_url")
        lamports = int(amount * 10**9) if amount is not None else None
        result_json = self.core.check_allowance(merchant_did, lamports)
        return json.loads(result_json)

    def risk_check(self, merchant_did: str, amount: float) -> dict:
        """Run a risk check combining blacklist and whitelist rules.

        Args:
            merchant_did: The merchant's DID.
            amount: Payment amount (natural units).

        Returns:
            dict with 'decision' (blocked/auto_approved/needs_auth) and optional details.
        """
        if not self.core:
            raise ValueError("Local mode requires mediator_url")
        lamports = int(amount * 10**9)
        result_json = self.core.risk_check(merchant_did, lamports)
        return json.loads(result_json)

    def add_to_whitelist(self, did: str, name: str = None, max_amount: float = None, label: str = None) -> None:
        """Add a merchant to the whitelist.

        Args:
            did: Merchant DID.
            name: Optional human-readable name.
            max_amount: Optional maximum allowed amount (natural units).
            label: Optional label/category.
        """
        if not self.core:
            raise ValueError("Local mode requires mediator_url")
        lamports = int(max_amount * 10**9) if max_amount is not None else None
        self.core.add_to_whitelist(did, name, lamports, label)

    def remove_from_whitelist(self, did: str) -> None:
        """Remove a merchant from the whitelist."""
        if not self.core:
            raise ValueError("Local mode requires mediator_url")
        self.core.remove_from_whitelist(did)

    def add_to_blacklist(self, did: str, name: str = None) -> None:
        """Add a merchant to the blacklist."""
        if not self.core:
            raise ValueError("Local mode requires mediator_url")
        self.core.add_to_blacklist(did, name)

    def remove_from_blacklist(self, did: str) -> None:
        """Remove a merchant from the blacklist."""
        if not self.core:
            raise ValueError("Local mode requires mediator_url")
        self.core.remove_from_blacklist(did)

    @property
    def our_did(self) -> str:
        """Return this agent's DID."""
        if not self.core:
            raise ValueError("Local mode requires mediator_url")
        return self.core.our_did
