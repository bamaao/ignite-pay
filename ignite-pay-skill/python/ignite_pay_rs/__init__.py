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
"""

from ignite_pay_rs.ignite_pay_rs import IgnitePayCore

__all__ = ["IgnitePayCore"]
