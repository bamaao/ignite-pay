# openclaw_skill.py
import asyncio
from ignite_pay_rs import IgnitePayCore

class IgnitePaySkill:
    def __init__(self):
        self.core = IgnitePayCore()
        # 启动后台监听
        self.core.start_listener("wss://mediator.ignite.com")

    async def pay_merchant(self, merchant_did, amount):
        # 这里的 await 会等待 Rust 内部的 oneshot 信号
        try:
            tx_sig = await self.core.check_and_pay(merchant_did, int(amount * 10**9))
            return f"支付成功: {tx_sig}"
        except Exception as e:
            return f"支付失败: {str(e)}"