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
"""

from ignite_pay_rs.ignite_pay_rs import IgnitePayCore

__all__ = ["IgnitePayCore"]
