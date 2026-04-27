"""
Devnet airdrop helper for funding session keys.

Usage:
    python fund_session.py <session_pubkey_base58> [amount_sol]

Defaults to 0.5 SOL airdrop. Requires Solana devnet to be responsive
(devnet airdrops are rate-limited).
"""

import sys

from solana.rpc.api import Client as SolanaClient
from solana.rpc.commitment import Confirmed
from solana.rpc.core import RPCException


def main():
    if len(sys.argv) < 2:
        print("Usage: python fund_session.py <session_pubkey_base58> [amount_sol]")
        print("  Defaults to 0.5 SOL airdrop on devnet.")
        sys.exit(1)

    pubkey = sys.argv[1]
    amount_sol = float(sys.argv[2]) if len(sys.argv) > 2 else 0.5
    amount_lamports = int(amount_sol * 1_000_000_000)

    rpc_url = "https://api.devnet.solana.com"
    client = SolanaClient(rpc_url)

    print(f"Requesting {amount_sol} SOL airdrop for {pubkey} on devnet...")

    try:
        resp = client.request_airdrop(pubkey, amount_lamports)
        tx_sig = resp.value
        print(f"Airdrop tx: {tx_sig}")
    except RPCException as e:
        print(f"Airdrop failed: {e}")
        sys.exit(1)

    print("Waiting for confirmation...")
    client.confirm_transaction(tx_sig, commitment=Confirmed)

    balance_resp = client.get_balance(pubkey, commitment=Confirmed)
    balance_sol = balance_resp.value / 1_000_000_000
    print(f"New balance: {balance_sol} SOL")


if __name__ == "__main__":
    main()
