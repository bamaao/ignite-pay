"""
Integration test for DIDComm Mediator WebSocket endpoint.
Tests: connect, mediate-request/grant, keylist-update, forward (online delivery), batch-pickup.
"""
import asyncio
import json
import sys
import websockets

MEDIATOR_WS = "ws://localhost:8080/ws"
ALICE_DID = "did:test:alice"
BOB_DID = "did:test:bob"
ALICE_KEY = "did:test:alice#key-1"


async def test_websocket():
    print("=" * 60)
    print("DIDComm Mediator WebSocket Integration Test")
    print("=" * 60)

    async with websockets.connect(MEDIATOR_WS) as ws:
        print("\n[1] Connected to", MEDIATOR_WS)

        # ---- Test 1: mediate-request ----
        print("\n[2] Sending mediate-request...")
        mediate_req = {
            "id": "msg-mediate-1",
            "type": "https://didcomm.org/coordinate-mediation/2.0/mediate-request",
            "from": ALICE_DID,
            "body": {}
        }
        await ws.send(json.dumps(mediate_req))
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        grant = json.loads(resp)
        print("   <- Received:", json.dumps(grant, indent=2))

        assert grant["type"] == "https://didcomm.org/coordinate-mediation/2.0/mediate-grant", \
            f"Expected mediate-grant, got {grant['type']}"
        assert grant["thid"] == "msg-mediate-1", "Thread ID mismatch"
        assert grant["from"] is not None, "Grant should have a 'from' (mediator DID)"
        assert ALICE_DID in (grant.get("to") or []), "Grant 'to' should include Alice"
        print("   PASS: mediate-grant received correctly")

        # ---- Test 2: keylist-update (add) ----
        print("\n[3] Sending keylist-update (add)...")
        keylist_req = {
            "id": "msg-keylist-1",
            "type": "https://didcomm.org/coordinate-mediation/2.0/keylist-update",
            "from": ALICE_DID,
            "body": {
                "updates": [
                    {"recipient_key": ALICE_KEY, "action": "add"}
                ]
            }
        }
        await ws.send(json.dumps(keylist_req))
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        keylist_resp = json.loads(resp)
        print("   <- Received:", json.dumps(keylist_resp, indent=2))

        assert keylist_resp["type"] == "https://didcomm.org/coordinate-mediation/2.0/keylist-update-response", \
            f"Expected keylist-update-response, got {keylist_resp['type']}"
        updated = keylist_resp["body"]["updated"]
        assert len(updated) == 1, f"Expected 1 update result, got {len(updated)}"
        assert updated[0]["result"] == "success", f"Expected success, got {updated[0]['result']}"
        print("   PASS: keylist-update-response received, key added")

        # ---- Test 3: forward message (online delivery) ----
        print("\n[4] Sending forward message (Alice is online)...")
        forward_msg = {
            "id": "msg-fwd-1",
            "type": "https://didcomm.org/routing/2.0/forward",
            "body": {
                "next": ALICE_KEY
            },
            "attachments": [{
                "data": {
                    "json": {"ciphertext": "fake-encrypted-payload-for-testing"}
                }
            }]
        }
        await ws.send(json.dumps(forward_msg))
        # Since Alice is online, the mediator should forward the inner message back to us
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        delivered = json.loads(resp)
        print("   <- Received forwarded message:", json.dumps(delivered, indent=2))

        assert delivered.get("ciphertext") == "fake-encrypted-payload-for-testing", \
            "Should receive the inner forwarded payload"
        print("   PASS: Forward message delivered online to connected client")

        # ---- Test 4: status-request ----
        print("\n[5] Sending status-request...")
        status_req = {
            "id": "msg-status-1",
            "type": "https://didcomm.org/messagepickup/3.0/status-request",
            "from": ALICE_DID,
            "body": {}
        }
        await ws.send(json.dumps(status_req))
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        status = json.loads(resp)
        print("   <- Received:", json.dumps(status, indent=2))

        assert status["type"] == "https://didcomm.org/messagepickup/3.0/status", \
            f"Expected status, got {status['type']}"
        assert status["body"]["message_count"] == 0, \
            f"Expected 0 queued messages, got {status['body']['message_count']}"
        print("   PASS: status shows 0 queued messages (all delivered online)")

        # ---- Test 5: batch-pickup (empty) ----
        print("\n[6] Sending batch-pickup...")
        batch_req = {
            "id": "msg-batch-1",
            "type": "https://didcomm.org/messagepickup/3.0/batch-pickup",
            "from": ALICE_DID,
            "body": {"count": 10}
        }
        await ws.send(json.dumps(batch_req))
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        batch = json.loads(resp)
        print("   <- Received:", json.dumps(batch, indent=2))

        assert batch["type"] == "https://didcomm.org/messagepickup/3.0/batch", \
            f"Expected batch, got {batch['type']}"
        assert len(batch["body"]["messages"]) == 0, "No messages should be queued"
        print("   PASS: batch-pickup returns empty (all delivered online)")

    print("\n" + "=" * 60)
    print("ALL TESTS PASSED")
    print("=" * 60)


async def test_forward_queuing():
    """Test that forward messages get queued when recipient is offline."""
    print("\n" + "=" * 60)
    print("Testing Forward Queuing (recipient offline)")
    print("=" * 60)

    # Step 1: Connect Carol, register keylist, then disconnect
    async with websockets.connect(MEDIATOR_WS) as ws:
        # Use mediate-request to identify (it registers the session via 'from')
        mediate_req = {
            "id": "carol-mediate-1",
            "type": "https://didcomm.org/coordinate-mediation/2.0/mediate-request",
            "from": "did:test:carol",
            "body": {}
        }
        await ws.send(json.dumps(mediate_req))
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        print("   Carol connected, got:", json.loads(resp).get("type"))

        # Register keylist
        keylist_req = {
            "id": "msg-kl",
            "type": "https://didcomm.org/coordinate-mediation/2.0/keylist-update",
            "from": "did:test:carol",
            "body": {
                "updates": [
                    {"recipient_key": "did:test:carol#key-1", "action": "add"}
                ]
            }
        }
        await ws.send(json.dumps(keylist_req))
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        print("   Keylist registered:", json.loads(resp).get("type"))

    # Carol is now disconnected
    print("   Carol disconnected")

    # Step 2: Send a forward message for Carol from a separate connection
    async with websockets.connect(MEDIATOR_WS) as ws:
        forward_msg = {
            "id": "msg-fwd-queued",
            "type": "https://didcomm.org/routing/2.0/forward",
            "body": {
                "next": "did:test:carol#key-1"
            },
            "attachments": [{
                "data": {
                    "json": {"ciphertext": "queued-for-carol"}
                }
            }]
        }
        await ws.send(json.dumps(forward_msg))
        print("   Forward message sent for offline Carol")
        await asyncio.sleep(0.5)

    # Step 3: Carol reconnects and picks up queued message
    async with websockets.connect(MEDIATOR_WS) as ws:
        # Re-identify with mediate-request
        mediate_req = {
            "id": "carol-mediate-2",
            "type": "https://didcomm.org/coordinate-mediation/2.0/mediate-request",
            "from": "did:test:carol",
            "body": {}
        }
        await ws.send(json.dumps(mediate_req))
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        print("   Carol reconnected, got:", json.loads(resp).get("type"))

        # Re-register keylist (new session)
        keylist_req = {
            "id": "msg-kl2",
            "type": "https://didcomm.org/coordinate-mediation/2.0/keylist-update",
            "from": "did:test:carol",
            "body": {
                "updates": [
                    {"recipient_key": "did:test:carol#key-1", "action": "add"}
                ]
            }
        }
        await ws.send(json.dumps(keylist_req))
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        print("   Keylist re-registered:", json.loads(resp).get("type"))

        # Status request to check queued count
        status_req = {
            "id": "msg-status-carol",
            "type": "https://didcomm.org/messagepickup/3.0/status-request",
            "from": "did:test:carol",
            "body": {}
        }
        await ws.send(json.dumps(status_req))
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        status = json.loads(resp)
        print("   Status:", json.dumps(status["body"], indent=2))
        assert status["body"]["message_count"] == 1, \
            f"Expected 1 queued message, got {status['body']['message_count']}"

        # Batch pickup
        batch_req = {
            "id": "msg-batch-carol",
            "type": "https://didcomm.org/messagepickup/3.0/batch-pickup",
            "from": "did:test:carol",
            "body": {"count": 10}
        }
        await ws.send(json.dumps(batch_req))
        resp = await asyncio.wait_for(ws.recv(), timeout=5)
        batch = json.loads(resp)
        print("   Batch:", json.dumps(batch["body"], indent=2))
        assert len(batch["body"]["messages"]) == 1, "Should have 1 queued message"
        inner = json.loads(batch["body"]["messages"][0]["message"])
        assert inner["ciphertext"] == "queued-for-carol"

    print("\n   PASS: Forward queuing and batch pickup work correctly!")
    print("=" * 60)


if __name__ == "__main__":
    try:
        asyncio.run(test_websocket())
        asyncio.run(test_forward_queuing())
    except Exception as e:
        print(f"\nTEST FAILED: {e}", file=sys.stderr)
        sys.exit(1)
