"""Ignite Pay Skill - Python SDK for agent-initiated payments."""
import json
from ignite_pay_rs import IgnitePayCore


class IgnitePaySkill:
    """High-level SDK for agent-initiated payments via Ignite Pay.

    Usage:
        skill = IgnitePaySkill(mediator_url="wss://mediator.ignite.com")
        result = await skill.pay_merchant("did:ignite:z...", 10.0)
    """

    def __init__(self, mediator_url: str, db_path: str = None):
        """Initialize the payment skill.

        Args:
            mediator_url: Mediator WebSocket URL, e.g. "wss://mediator.ignite.com"
            db_path: Whitelist/blacklist persistence path (optional, omit for memory-only)
        """
        self.core = IgnitePayCore()
        if db_path:
            self.core.init_list_store(db_path)
        self.core.start_listener(mediator_url)
        self.mediator_url = mediator_url

    async def pay_merchant(self, merchant_did: str, amount: float, reason: str = "Payment") -> dict:
        """Execute a payment. Auto-generates payment_id, sends auth request, waits for phone confirmation.

        Args:
            merchant_did: The merchant's DID identifier.
            amount: Payment amount (will be converted to lamports: amount * 10^9).
            reason: Human-readable payment description.

        Returns:
            dict with 'success' and 'tx_sig' or 'error'.
        """
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
        lamports = int(max_amount * 10**9) if max_amount is not None else None
        self.core.add_to_whitelist(did, name, lamports, label)

    def remove_from_whitelist(self, did: str) -> None:
        """Remove a merchant from the whitelist."""
        self.core.remove_from_whitelist(did)

    def add_to_blacklist(self, did: str, name: str = None) -> None:
        """Add a merchant to the blacklist."""
        self.core.add_to_blacklist(did, name)

    def remove_from_blacklist(self, did: str) -> None:
        """Remove a merchant from the blacklist."""
        self.core.remove_from_blacklist(did)

    @property
    def our_did(self) -> str:
        """Return this agent's DID."""
        return self.core.our_did
