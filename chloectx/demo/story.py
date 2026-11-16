"""The demo's story: three and a half weeks of Claude Code sessions on
"harbor", a mock order and payments service, for `demo/seed.py`.

FILES is the repository at its first commit. Each session is a list of
turns: the developer's prompt, the tool calls Claude Code made (each one
applied to the files, so that blame finds the lines it wrote), its answer,
and what the distiller writes down from the turn (`notes`: a decision, rule,
requirement or fact; `replaces` names the start of the note it updates,
`done` the start of a requirement it completes). The scripted distiller
(`demo/scripted_distill.py`) answers with those notes.

Nothing here is a real company, person or system.
"""

FILES = {
    "README.md": """# harbor

The order and payments API of the shop: orders, payments and refunds.
FastAPI on Python 3.12, Postgres 16, Stripe for card payments.

    uv sync
    make test      # pytest against the Postgres of docker compose
    make run       # http://127.0.0.1:8000/docs
""",
    "pyproject.toml": """[project]
name = "harbor"
version = "0.14.0"
requires-python = ">=3.12"
dependencies = [
    "fastapi>=0.115",
    "uvicorn>=0.30",
    "sqlalchemy>=2.0",
    "psycopg[binary]>=3.2",
    "stripe>=10.0",
    "structlog>=24.1",
    "redis>=5.0",
]

[dependency-groups]
dev = ["pytest>=8.2", "httpx>=0.27", "mypy>=1.10"]
""",
    "Makefile": """test:
\tdocker compose up -d db
\tuv run pytest -q

run:
\tuv run uvicorn app.main:app --reload
""",
    "docker-compose.yml": """services:
  db:
    image: postgres:16
    ports: ["5433:5432"]
    environment:
      POSTGRES_PASSWORD: harbor
      POSTGRES_DB: harbor_test
  redis:
    image: redis:7
    ports: ["6380:6379"]
""",
    "app/__init__.py": "",
    "app/config.py": """from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    database_url: str = "postgresql+psycopg://postgres:harbor@localhost:5433/harbor_test"
    stripe_api_key: str = ""
    redis_url: str = "redis://localhost:6380/0"


settings = Settings()
""",
    "app/main.py": """from fastapi import FastAPI

from app.orders.routes import router as orders
from app.webhooks import router as webhooks

app = FastAPI(title="harbor")
app.include_router(orders)
app.include_router(webhooks)
""",
    "app/payments/__init__.py": "",
    "app/payments/client.py": '''"""Thin wrapper around the Stripe SDK."""
import stripe

from app.config import settings


class PaymentsClient:
    def __init__(self, api_key: str | None = None):
        self._stripe = stripe
        self._stripe.api_key = api_key or settings.stripe_api_key

    def charge(self, amount: int, currency: str, customer_id: str, idempotency_key: str):
        return self._stripe.PaymentIntent.create(
            amount=amount,
            currency=currency,
            customer=customer_id,
            idempotency_key=idempotency_key,
        )
''',
    "app/orders/__init__.py": "",
    "app/orders/models.py": """from datetime import datetime

from sqlalchemy import ForeignKey, String
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column, relationship


class Base(DeclarativeBase):
    pass


class Order(Base):
    __tablename__ = "orders"

    id: Mapped[int] = mapped_column(primary_key=True)
    customer_id: Mapped[str] = mapped_column(String(64), index=True)
    total_cents: Mapped[int]
    status: Mapped[str] = mapped_column(String(16), default="paid")
    payment_intent: Mapped[str] = mapped_column(String(64))
    created_at: Mapped[datetime]
    items: Mapped[list["OrderItem"]] = relationship(back_populates="order")


class OrderItem(Base):
    __tablename__ = "order_items"

    id: Mapped[int] = mapped_column(primary_key=True)
    order_id: Mapped[int] = mapped_column(ForeignKey("orders.id"))
    sku: Mapped[str] = mapped_column(String(32))
    quantity: Mapped[int]
    price_cents: Mapped[int]
    order: Mapped[Order] = relationship(back_populates="items")
""",
    "app/orders/routes.py": """from fastapi import APIRouter, Depends
from sqlalchemy import select
from sqlalchemy.orm import Session

from app.db import session
from app.orders.models import Order

router = APIRouter(prefix="/orders")


@router.get("")
def list_orders(customer_id: str, db: Session = Depends(session)):
    orders = db.scalars(select(Order).where(Order.customer_id == customer_id).limit(100)).all()
    return [{"id": o.id, "total_cents": o.total_cents, "items": [i.sku for i in o.items]} for o in orders]


@router.get("/{order_id}")
def get_order(order_id: int, db: Session = Depends(session)):
    return db.get(Order, order_id)
""",
    "app/db.py": """from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker

from app.config import settings

engine = create_engine(settings.database_url)
Session = sessionmaker(engine)


def session():
    with Session() as s:
        yield s
""",
    "app/webhooks.py": """import json

from fastapi import APIRouter, Request

router = APIRouter()


@router.post("/webhooks/stripe")
async def stripe_webhook(request: Request):
    event = json.loads(await request.body())
    if event["type"] == "charge.refunded":
        pass  # mark the refund as settled
    return {"ok": True}
""",
    "app/reports/__init__.py": "",
    "app/reports/settlement.py": """from datetime import date, datetime, time

from sqlalchemy import func, select

from app.orders.models import Order


def daily_totals(db, day: date):
    start = datetime.combine(day, time.min)
    end = datetime.combine(day, time.max)
    q = select(func.count(), func.sum(Order.total_cents)).where(Order.created_at.between(start, end))
    count, total = db.execute(q).one()
    return {"day": day.isoformat(), "orders": count, "total_cents": total or 0}
""",
    "migrations/0006_order_items.sql": """CREATE TABLE order_items (
    id BIGSERIAL PRIMARY KEY,
    order_id BIGINT NOT NULL REFERENCES orders(id),
    sku VARCHAR(32) NOT NULL,
    quantity INT NOT NULL,
    price_cents BIGINT NOT NULL
);
""",
    "tests/__init__.py": "",
    "tests/test_orders.py": """def test_list_orders(client, order):
    r = client.get("/orders", params={"customer_id": order.customer_id})
    assert r.status_code == 200
    assert r.json()[0]["id"] == order.id
""",
    "docs/runbook.md": """# harbor runbook

## Deploys

Merges to main build an image; staging takes it within ten minutes.
""",
    ".github/workflows/ci.yml": """name: CI
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    services:
      db:
        image: postgres:16
        env: { POSTGRES_PASSWORD: harbor, POSTGRES_DB: harbor_test }
        ports: ["5433:5432"]
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v3
      - run: uv sync
      - run: uv run pytest -q
""",
}


def T(prompt, answer, tools=(), notes=(), gap=6):
    return {"prompt": prompt, "answer": answer, "tools": list(tools), "notes": list(notes), "gap": gap}


def N(kind, text, entities=(), replaces=None, when=()):
    n = {"kind": kind, "text": text, "entities": list(entities), "when": list(when)}
    if replaces:
        n["replaces"] = replaces
    return n


def DONE(start):
    return {"kind": "done", "done": start}


def tests(n, secs="3.4", extra=""):
    return ("bash", "make test", f"docker compose up -d db\n[+] Running 1/0\n ✔ Container harbor-db-1  Running\nuv run pytest -q\n{'.' * min(n, 60)}{extra}\n{n} passed in {secs}s", 0)


SESSIONS = [
    # ---------------------------------------------------------------- week 1
    {"when": "2026-10-19 09:12", "turns": [
        T("I'm new to this repo. Give me the lay of the land: what does harbor do, how is it laid out, and how do I run the tests?",
          "harbor is the shop's order and payments API: FastAPI on Python 3.12 with SQLAlchemy 2 over Postgres 16, and Stripe for card payments.\n\n"
          "- `app/orders/`: the Order and OrderItem models and the `/orders` routes\n- `app/payments/client.py`: `PaymentsClient`, a thin wrapper over the Stripe SDK (only `charge` so far)\n"
          "- `app/webhooks.py`: the Stripe webhook endpoint, which does nothing yet\n- `app/reports/settlement.py`: daily totals for finance\n- `migrations/`: plain SQL files, numbered (the latest is 0006)\n\n"
          "`make test` starts the Postgres of docker compose (port 5433) and runs pytest: 14 tests pass. Prices and payments are all in EUR: there is no currency column anywhere.",
          tools=[("read", "README.md"), ("read", "pyproject.toml"), ("read", "Makefile"),
                 ("bash", "ls app app/orders app/payments migrations", "app:\n__init__.py  config.py  db.py  main.py  orders  payments  reports  webhooks.py\n\napp/orders:\n__init__.py  models.py  routes.py\n\napp/payments:\n__init__.py  client.py\n\nmigrations:\n0001_init.sql  0002_customers.sql  0003_orders.sql  0004_payments.sql  0005_order_status.sql  0006_order_items.sql", 0),
                 ("read", "app/payments/client.py"), ("grep", "currency", "app", "app/payments/client.py:12:    def charge(self, amount: int, currency: str, customer_id: str, idempotency_key: str):"),
                 tests(14)],
          notes=[N("fact", "harbor is the shop's order and payments API: FastAPI on Python 3.12, SQLAlchemy 2 over Postgres 16, and Stripe for card payments.", ["harbor", "Stripe", "Postgres"]),
                 N("fact", "make test starts the Postgres of docker compose (port 5433) and runs pytest; the first run pulls the postgres:16 image.", ["make test", "docker compose"], when=["running the tests"]),
                 N("fact", "All prices and payments are in EUR: orders have no currency column.", ["currency", "orders"])]),
        T("Use uv for everything, never pip. And don't add a dependency without asking me first, the security review for new packages takes a week.",
          "Got it: uv for installs and commands (`uv sync`, `uv run`, `uv add`), and I'll ask before adding anything to pyproject.toml.",
          notes=[N("rule", "Use uv for installs and commands (uv sync, uv run, uv add), never pip.", ["uv"]),
                 N("rule", "Ask before adding a dependency to pyproject.toml: a new package goes through a security review that takes a week.", ["dependencies", "pyproject.toml"], when=["adding a library"])]),
    ]},
    {"when": "2026-10-20 10:05", "turns": [
        T("Add a POST /orders/{id}/refunds endpoint. Full refunds only for now.",
          "Added `POST /orders/{order_id}/refunds` in `app/orders/routes.py`. It loads the order, refuses one that is not paid (409), and creates the refund on Stripe for the order's total with the request's Idempotency-Key, then marks the order refunded.",
          tools=[("read", "app/orders/routes.py"), ("read", "app/orders/models.py"), ("grep", "refund", "app", "app/webhooks.py:11:    if event[\"type\"] == \"charge.refunded\":"),
                 ("edit", "app/orders/routes.py", "from fastapi import APIRouter, Depends\n", "import stripe\nfrom fastapi import APIRouter, Depends, Header, HTTPException\n"),
                 ("edit", "app/orders/routes.py", "@router.get(\"/{order_id}\")\n", "@router.post(\"/{order_id}/refunds\")\ndef refund_order(order_id: int, idempotency_key: str = Header(...), db: Session = Depends(session)):\n    order = db.get(Order, order_id)\n    if order is None or order.status != \"paid\":\n        raise HTTPException(409, \"only a paid order can be refunded\")\n    refund = stripe.Refund.create(payment_intent=order.payment_intent, amount=order.total_cents / 100, idempotency_key=idempotency_key)\n    order.status = \"refunded\"\n    db.commit()\n    return {\"refund_id\": refund.id}\n\n\n@router.get(\"/{order_id}\")\n")]),
        T("No. Never call stripe from a route. Everything that talks to Stripe goes through PaymentsClient, that's the whole point of the wrapper. And what is `order.total_cents / 100`? Stripe takes cents, and we never use floats for money here.",
          "You're right on both. I moved the call into `PaymentsClient.refund()` and a `RefundService` in `app/payments/refunds.py`; the route now only parses the request and calls the service. The amount is the order's `total_cents` as an int, passed to Stripe unchanged.",
          tools=[("edit", "app/payments/client.py", "            idempotency_key=idempotency_key,\n        )\n", "            idempotency_key=idempotency_key,\n        )\n\n    def refund(self, payment_intent: str, amount_cents: int, idempotency_key: str):\n        return self._stripe.Refund.create(\n            payment_intent=payment_intent,\n            amount=amount_cents,\n            idempotency_key=idempotency_key,\n        )\n"),
                 ("write", "app/payments/refunds.py", "\"\"\"Refunds: every refund of an order goes through RefundService.\"\"\"\nfrom app.orders.models import Order\nfrom app.payments.client import PaymentsClient\n\n\nclass RefundError(Exception):\n    pass\n\n\nclass RefundService:\n    def __init__(self, db, client: PaymentsClient | None = None):\n        self.db = db\n        self.client = client or PaymentsClient()\n\n    def refund(self, order_id: int, idempotency_key: str) -> str:\n        order = self.db.get(Order, order_id)\n        if order is None or order.status != \"paid\":\n            raise RefundError(\"only a paid order can be refunded\")\n        refund = self.client.refund(order.payment_intent, order.total_cents, idempotency_key)\n        order.status = \"refunded\"\n        self.db.commit()\n        return refund.id\n"),
                 ("edit", "app/orders/routes.py", "import stripe\nfrom fastapi import APIRouter, Depends, Header, HTTPException\n", "from fastapi import APIRouter, Depends, Header, HTTPException\n"),
                 ("edit", "app/orders/routes.py", "    order = db.get(Order, order_id)\n    if order is None or order.status != \"paid\":\n        raise HTTPException(409, \"only a paid order can be refunded\")\n    refund = stripe.Refund.create(payment_intent=order.payment_intent, amount=order.total_cents / 100, idempotency_key=idempotency_key)\n    order.status = \"refunded\"\n    db.commit()\n    return {\"refund_id\": refund.id}\n",
                  "    try:\n        refund_id = RefundService(db).refund(order_id, idempotency_key)\n    except RefundError as e:\n        raise HTTPException(409, str(e))\n    return {\"refund_id\": refund_id}\n"),
                 ("edit", "app/orders/routes.py", "from app.orders.models import Order\n", "from app.orders.models import Order\nfrom app.payments.refunds import RefundError, RefundService\n")],
          notes=[N("rule", "Never call the Stripe SDK outside app/payments/client.py: routes and services go through PaymentsClient.", ["Stripe", "PaymentsClient"], when=["calling Stripe"]),
                 N("rule", "Money is integer minor units (amount_cents) everywhere, passed to Stripe unchanged; never floats or division by 100.", ["money", "amount_cents"]),
                 N("decision", "Refunds go through RefundService in app/payments/refunds.py; POST /orders/{id}/refunds requires an Idempotency-Key header.", ["refunds", "RefundService", "Idempotency-Key"])]),
        T("Good. Write the tests for it.",
          "Added `tests/test_refunds.py`: a paid order is refunded once, a second call with the same key returns the same refund, an unpaid order gets 409, and a missing Idempotency-Key gets 422. Stripe is a fake `PaymentsClient` in the fixture. 18 tests pass.",
          tools=[("write", "tests/test_refunds.py", "from app.payments.refunds import RefundError, RefundService\n\n\ndef test_refund_paid_order(db, order, fake_client):\n    rid = RefundService(db, fake_client).refund(order.id, \"k1\")\n    assert rid == fake_client.refunds[0].id\n    assert order.status == \"refunded\"\n\n\ndef test_refund_needs_a_paid_order(db, unpaid_order, fake_client):\n    try:\n        RefundService(db, fake_client).refund(unpaid_order.id, \"k2\")\n    except RefundError:\n        return\n    raise AssertionError(\"refunded an unpaid order\")\n\n\ndef test_refund_route_needs_a_key(client, order):\n    assert client.post(f\"/orders/{order.id}/refunds\").status_code == 422\n"),
                 tests(18, "3.9")]),
        T("Finance will want partial refunds at some point, not now. Note it.",
          "Noted as planned work: partial refunds, an amount below what is left to refund on the order.",
          notes=[N("requirement", "Partial refunds: POST /orders/{id}/refunds should take an amount below the order's total (finance asked; not now).", ["partial refunds", "refunds"])]),
        T("commit it", "Committed as \"Refunds through RefundService and PaymentsClient\".", tools=[("commit", "Refunds through RefundService and PaymentsClient")], gap=2),
    ]},
    {"when": "2026-10-21 14:30", "turns": [
        T("Stripe webhooks are failing in staging with 400s since this morning's deploy. Can you look?",
          "The 400s come from the load balancer, not from us: since this morning's deploy it requires a `Stripe-Signature` check that our endpoint never made, so it rejects the requests our app answers with 200. I added the check where the rule says Stripe calls go: `PaymentsClient.verify_webhook()` calls `stripe.Webhook.construct_event` with `STRIPE_WEBHOOK_SECRET`, and the endpoint answers 400 itself on a bad signature. Staging needs the secret of its own webhook endpoint in its environment.",
          tools=[("read", "app/webhooks.py"), ("bash", "kubectl -n staging logs deploy/harbor --since=6h | grep webhooks | tail -5", "2026-10-21T13:58:02Z POST /webhooks/stripe 400 signature verification failed: no Stripe-Signature check configured\n2026-10-21T14:01:17Z POST /webhooks/stripe 400 signature verification failed: no Stripe-Signature check configured", 0),
                 ("grep", "WEBHOOK", "app", ""),
                 ("edit", "app/config.py", "    stripe_api_key: str = \"\"\n", "    stripe_api_key: str = \"\"\n    stripe_webhook_secret: str = \"\"\n"),
                 ("edit", "app/payments/client.py", "    def refund(self, payment_intent: str, amount_cents: int, idempotency_key: str):\n", "    def verify_webhook(self, payload: bytes, signature: str):\n        return self._stripe.Webhook.construct_event(payload, signature, settings.stripe_webhook_secret, tolerance=300)\n\n    def refund(self, payment_intent: str, amount_cents: int, idempotency_key: str):\n"),
                 ("write", "app/webhooks.py", "import stripe\nfrom fastapi import APIRouter, Header, HTTPException, Request\n\nfrom app.payments.client import PaymentsClient\n\nrouter = APIRouter()\n\n\n@router.post(\"/webhooks/stripe\")\nasync def stripe_webhook(request: Request, stripe_signature: str = Header(...)):\n    try:\n        event = PaymentsClient().verify_webhook(await request.body(), stripe_signature)\n    except stripe.SignatureVerificationError:\n        raise HTTPException(400, \"bad signature\")\n    if event[\"type\"] == \"charge.refunded\":\n        pass  # mark the refund as settled\n    return {\"ok\": True}\n")],
          notes=[N("fact", "Stripe webhook signatures are checked with STRIPE_WEBHOOK_SECRET (whsec_...); staging and production each have their own endpoint and secret.", ["Stripe", "webhooks", "STRIPE_WEBHOOK_SECRET"]),
                 N("decision", "Webhook events more than 5 minutes old are rejected (signature tolerance 300 s), against replays.", ["webhooks", "replay"])]),
        T("Add a test that an event older than five minutes is rejected.",
          "Added `test_webhook_rejects_old_event`: it signs an event dated six minutes ago and expects 400. 19 tests pass.",
          tools=[("write", "tests/test_webhooks.py", "import time\n\nfrom tests.helpers import signed\n\n\ndef test_webhook_rejects_old_event(client):\n    body, sig = signed({\"type\": \"charge.refunded\"}, at=time.time() - 360)\n    r = client.post(\"/webhooks/stripe\", content=body, headers={\"Stripe-Signature\": sig})\n    assert r.status_code == 400\n"),
                 tests(19, "4.1")]),
        T("commit", "Committed as \"Verify Stripe webhook signatures\".", tools=[("commit", "Verify Stripe webhook signatures")], gap=1),
    ]},
    {"when": "2026-10-22 11:20", "turns": [
        T("Finance wants a second pair of eyes on big refunds: anything over 500 EUR needs an approval from someone in the finance group before it goes to Stripe.",
          "Refunds now check `APPROVAL_LIMIT_CENTS = 50_000` in `RefundService`: above it, the refund is kept as pending (status `awaiting_approval`) instead of being sent, and the route answers 202 with the approval's id. At or below 500 EUR nothing changes.",
          tools=[("read", "app/payments/refunds.py"),
                 ("edit", "app/payments/refunds.py", "class RefundError(Exception):\n    pass\n", "APPROVAL_LIMIT_CENTS = 50_000  # 500 EUR: above it, finance approves first\n\n\nclass RefundError(Exception):\n    pass\n\n\nclass NeedsApproval(Exception):\n    def __init__(self, approval_id: int):\n        self.approval_id = approval_id\n"),
                 ("edit", "app/payments/refunds.py", "        refund = self.client.refund(order.payment_intent, order.total_cents, idempotency_key)\n", "        if order.total_cents > APPROVAL_LIMIT_CENTS:\n            raise NeedsApproval(self._request_approval(order, idempotency_key))\n        refund = self.client.refund(order.payment_intent, order.total_cents, idempotency_key)\n"),
                 tests(20, "4.0")],
          notes=[N("decision", "Refunds over 500 EUR need a second approver from the finance group before they are sent to Stripe; smaller ones go straight through.", ["refunds", "approval", "finance"], when=["refunding an order"])]),
        T("Where do the pending approvals live?",
          "In a new table, `refund_approvals` (migration 0007): order, amount, idempotency key, who asked, who approved and when. Approving one (POST /refund-approvals/{id}/approve, finance group only) sends the refund with the stored key.",
          tools=[("write", "migrations/0007_refund_approvals.sql", "CREATE TABLE refund_approvals (\n    id BIGSERIAL PRIMARY KEY,\n    order_id BIGINT NOT NULL REFERENCES orders(id),\n    amount_cents BIGINT NOT NULL,\n    idempotency_key TEXT NOT NULL,\n    requested_by TEXT NOT NULL,\n    approved_by TEXT,\n    approved_at TIMESTAMPTZ\n);\n")],
          notes=[N("decision", "Pending refund approvals are rows of refund_approvals (migration 0007); approving one (finance group only) sends the refund with its stored idempotency key.", ["refund_approvals", "approval"])]),
        T("commit", "Committed as \"Finance approves refunds over 500 EUR\".", tools=[("commit", "Finance approves refunds over 500 EUR")], gap=2),
    ]},
    {"when": "2026-10-23 16:40", "turns": [
        T("The approvals need a reason column too, support wants to say why they refund.",
          "Added `reason TEXT NOT NULL DEFAULT ''` to `refund_approvals` in migration 0007.",
          tools=[("edit", "migrations/0007_refund_approvals.sql", "    approved_at TIMESTAMPTZ\n);\n", "    approved_at TIMESTAMPTZ,\n    reason TEXT NOT NULL DEFAULT ''\n);\n")]),
        T("Don't edit 0007, it was applied in staging yesterday. Migrations are forward only here: add 0008.",
          "Reverted 0007 to what was applied and added `migrations/0008_refund_reason.sql` with the `ALTER TABLE`.",
          tools=[("edit", "migrations/0007_refund_approvals.sql", "    approved_at TIMESTAMPTZ,\n    reason TEXT NOT NULL DEFAULT ''\n);\n", "    approved_at TIMESTAMPTZ\n);\n"),
                 ("write", "migrations/0008_refund_reason.sql", "ALTER TABLE refund_approvals ADD COLUMN reason TEXT NOT NULL DEFAULT '';\n"),
                 tests(20, "4.2")],
          notes=[N("rule", "Migrations are forward-only: never edit a migration that was merged or applied anywhere; add the next numbered file.", ["migrations"], when=["changing the schema"])]),
        T("commit", "Committed as \"Refund approvals have a reason\".", tools=[("commit", "Refund approvals have a reason")], gap=1),
    ]},
    # ---------------------------------------------------------------- week 2
    {"when": "2026-10-26 09:45", "turns": [
        T("test_webhook_rejects_old_event fails about one CI run in ten. Find out why.",
          "It is the clock. The test signs an event dated \"six minutes ago\", but `signed()` in tests/helpers.py truncates the timestamp to the whole minute, so the event is anywhere from five to six minutes old, and at five minutes the 300 s tolerance still accepts it: about one run in ten. Freezing time makes the age exact. That needs freezegun, a new dev dependency: may I add it?",
          tools=[("bash", "uv run pytest tests/test_webhooks.py -q --count 30", "..........F...............F...\nFAILED tests/test_webhooks.py::test_webhook_rejects_old_event - assert 200 == 400\n2 failed, 28 passed in 41.8s", 1),
                 ("read", "tests/test_webhooks.py"), ("read", "app/payments/client.py")],
          notes=[N("fact", "test_webhook_rejects_old_event was flaky (about 1 CI run in 10): tests/helpers.signed() truncates timestamps to the minute, so a six-minute-old event could be five minutes old.", ["flaky test", "webhooks", "CI"])]),
        T("Yes, add freezegun as a dev dependency.",
          "Added freezegun to the dev group with `uv add --dev freezegun` and froze time in the test. 30 runs in a row pass.",
          tools=[("bash", "uv add --dev freezegun", "Resolved 41 packages in 312ms\nInstalled 1 package in 4ms\n + freezegun==1.5.1", 0),
                 ("edit", "tests/test_webhooks.py", "import time\n\nfrom tests.helpers import signed\n\n\ndef test_webhook_rejects_old_event(client):\n    body, sig = signed({\"type\": \"charge.refunded\"}, at=time.time() - 360)\n",
                  "from freezegun import freeze_time\n\nfrom tests.helpers import signed\n\n\n@freeze_time(\"2026-10-26 09:00:00\")\ndef test_webhook_rejects_old_event(client):\n    body, sig = signed({\"type\": \"charge.refunded\"}, at=\"2026-10-26 08:54:00\")\n"),
                 ("bash", "uv run pytest tests/test_webhooks.py -q --count 30", "..............................\n30 passed in 39.2s", 0)],
          notes=[N("rule", "Tests that depend on time freeze it with freezegun's freeze_time; never sleep or read the real clock in a test.", ["tests", "freezegun", "time"], when=["writing a test with dates"])]),
        T("commit", "Committed as \"Freeze time in the webhook test\".", tools=[("commit", "Freeze time in the webhook test")], gap=1),
    ]},
    {"when": "2026-10-27 13:10", "turns": [
        T("We need rate limiting on the public API before the partner launch. Something simple.",
          "Added `app/rate_limit.py`: a token bucket per API key in Redis (one key per API key, refilled on read), 100 requests a minute with a burst of 20. Over it the API answers 429 with a Retry-After header. It is a middleware in `app/main.py`; the webhook path is exempt, since Stripe retries on 429 anyway.",
          tools=[("read", "app/main.py"), ("read", "app/config.py"),
                 ("write", "app/rate_limit.py", "\"\"\"A token bucket per API key, kept in Redis.\"\"\"\nimport time\n\nimport redis\nfrom fastapi import Request\nfrom fastapi.responses import JSONResponse\n\nfrom app.config import settings\n\nRATE_PER_MINUTE = 100\nBURST = 20\nEXEMPT = (\"/webhooks/\",)\n\n_redis = redis.Redis.from_url(settings.redis_url)\n\n\ndef allow(key: str, now: float | None = None) -> tuple[bool, float]:\n    now = now or time.time()\n    tokens, at = _redis.hmget(f\"rl:{key}\", \"tokens\", \"at\")\n    tokens = float(tokens or BURST)\n    at = float(at or now)\n    tokens = min(BURST, tokens + (now - at) * RATE_PER_MINUTE / 60)\n    ok = tokens >= 1\n    _redis.hset(f\"rl:{key}\", mapping={\"tokens\": tokens - ok, \"at\": now})\n    return ok, 0 if ok else (1 - tokens) * 60 / RATE_PER_MINUTE\n\n\nasync def middleware(request: Request, call_next):\n    if request.url.path.startswith(EXEMPT):\n        return await call_next(request)\n    ok, wait = allow(request.headers.get(\"x-api-key\", request.client.host))\n    if not ok:\n        return JSONResponse({\"error\": \"rate limited\"}, status_code=429, headers={\"Retry-After\": str(int(wait) + 1)})\n    return await call_next(request)\n"),
                 ("edit", "app/main.py", "app = FastAPI(title=\"harbor\")\n", "from app import rate_limit\n\napp = FastAPI(title=\"harbor\")\napp.middleware(\"http\")(rate_limit.middleware)\n"),
                 tests(23, "4.4")],
          notes=[N("decision", "Rate limiting is a token bucket per API key in Redis (app/rate_limit.py): 100 requests a minute, burst 20; over it the API answers 429 with Retry-After. Webhooks are exempt.", ["rate limiting", "Redis", "API keys"])]),
        T("commit", "Committed as \"Rate limit the public API\".", tools=[("commit", "Rate limit the public API")], gap=1),
    ]},
    {"when": "2026-10-28 10:02", "turns": [
        T("I saw a card fingerprint and a customer's email in the logs from the refund code. Fix it and make sure it can't happen again.",
          "The refund service logged the whole Stripe refund object, which carries the charge's billing details. It now logs only ids. To keep it from coming back, `app/logging.py` configures structlog with a processor that drops keys known to hold personal data (email, name, card, billing_details, fingerprint) from every event, and a test fails if one gets through.",
          tools=[("grep", "log\\.", "app/payments", "app/payments/refunds.py:31:        log.info(\"refund created\", refund=refund)"),
                 ("edit", "app/payments/refunds.py", "\"\"\"Refunds: every refund of an order goes through RefundService.\"\"\"\n", "\"\"\"Refunds: every refund of an order goes through RefundService.\"\"\"\nimport structlog\n\nlog = structlog.get_logger()\n\n"),
                 ("write", "app/logging.py", "\"\"\"structlog, as JSON, with personal data dropped from every event.\"\"\"\nimport structlog\n\nPII_KEYS = {\"email\", \"name\", \"card\", \"billing_details\", \"fingerprint\", \"phone\", \"address\"}\n\n\ndef drop_pii(_, __, event: dict) -> dict:\n    for k in list(event):\n        if k in PII_KEYS:\n            event[k] = \"[dropped]\"\n    return event\n\n\nstructlog.configure(processors=[drop_pii, structlog.processors.TimeStamper(fmt=\"iso\"), structlog.processors.JSONRenderer()])\n"),
                 tests(24, "4.3")],
          notes=[N("rule", "Never log card data, emails or names: log ids only (order_id, customer_id, refund_id); app/logging.py drops known personal-data keys from every event.", ["logging", "PII"], when=["adding a log line"]),
                 N("decision", "Logging is structlog with JSON output: log = structlog.get_logger() and log.info(event, **ids); no print() in app code.", ["logging", "structlog"])]),
        T("commit", "Committed as \"Keep personal data out of the logs\".", tools=[("commit", "Keep personal data out of the logs")], gap=1),
    ]},
    {"when": "2026-10-29 15:25", "turns": [
        T("We launch in the UK on December 1st, so orders can be in GBP. What breaks?",
          "Everything assumes EUR, since there is no currency column: the totals in the settlement report, the refund approval limit (50,000 cents is 500 EUR), and the charge call, which passes a currency the caller gives but stores nothing. I added `currency CHAR(3) NOT NULL DEFAULT 'EUR'` to orders (migration 0009), the model field, and made the approval limit per currency: 500 EUR or 450 GBP. The settlement report now groups by currency.",
          tools=[("grep", "EUR|total_cents", "app", "app/reports/settlement.py:12:    q = select(func.count(), func.sum(Order.total_cents))\napp/payments/refunds.py:9:APPROVAL_LIMIT_CENTS = 50_000  # 500 EUR: above it, finance approves first\napp/orders/models.py:16:    total_cents: Mapped[int]"),
                 ("write", "migrations/0009_order_currency.sql", "ALTER TABLE orders ADD COLUMN currency CHAR(3) NOT NULL DEFAULT 'EUR';\n"),
                 ("edit", "app/orders/models.py", "    total_cents: Mapped[int]\n    status", "    total_cents: Mapped[int]\n    currency: Mapped[str] = mapped_column(String(3), default=\"EUR\")\n    status"),
                 ("edit", "app/payments/refunds.py", "APPROVAL_LIMIT_CENTS = 50_000  # 500 EUR: above it, finance approves first\n", "# Above these, finance approves first.\nAPPROVAL_LIMIT_CENTS = {\"EUR\": 50_000, \"GBP\": 45_000}\n"),
                 ("edit", "app/payments/refunds.py", "        if order.total_cents > APPROVAL_LIMIT_CENTS:\n", "        if order.total_cents > APPROVAL_LIMIT_CENTS[order.currency]:\n"),
                 tests(26, "4.6")],
          notes=[N("decision", "Orders carry an ISO 4217 currency (EUR or GBP, migration 0009) next to total_cents; amounts are minor units of that currency.", ["currency", "GBP", "orders"], replaces="All prices and payments are in EUR"),
                 N("decision", "Refunds over 500 EUR or 450 GBP need a second approver from the finance group before they are sent to Stripe.", ["refunds", "approval", "finance", "GBP"], replaces="Refunds over 500 EUR need a second approver")]),
        T("commit", "Committed as \"Orders have a currency\".", tools=[("commit", "Orders have a currency")], gap=1),
    ]},
    {"when": "2026-10-30 11:50", "turns": [
        T("GET /orders takes 4 seconds for our biggest customers. Profile it.",
          "It is an N+1: the list loads the orders in one query, then each order's items in one query each when the response reads `o.items`. For a customer with 300 orders that is 301 queries. With `selectinload(Order.items)` it is 2 queries for any page; the 300-order customer now takes 85 ms.",
          tools=[("bash", "uv run python -m scripts.profile orders --customer cus_big", "GET /orders?customer_id=cus_big\n  queries: 301\n  time: 4.12 s", 0),
                 ("read", "app/orders/routes.py"),
                 ("edit", "app/orders/routes.py", "from sqlalchemy.orm import Session\n", "from sqlalchemy.orm import Session, selectinload\n"),
                 ("edit", "app/orders/routes.py", "    orders = db.scalars(select(Order).where(Order.customer_id == customer_id).limit(100)).all()\n", "    q = select(Order).where(Order.customer_id == customer_id).options(selectinload(Order.items)).limit(100)\n    orders = db.scalars(q).all()\n"),
                 ("bash", "uv run python -m scripts.profile orders --customer cus_big", "GET /orders?customer_id=cus_big\n  queries: 2\n  time: 0.085 s", 0),
                 tests(26, "4.5")],
          notes=[N("fact", "GET /orders made one query per order for its items (N+1, 4 s for 300 orders); it loads them with selectinload now: 2 queries, 85 ms.", ["GET /orders", "N+1", "performance"])]),
        T("commit", "Committed as \"Load order items in one query\".", tools=[("commit", "Load order items in one query")], gap=1),
    ]},
    # ---------------------------------------------------------------- week 3
    {"when": "2026-11-02 09:30", "turns": [
        T("Update from finance: the approval limit is 1000 EUR, not 500. They say 500 makes too much work for them. GBP goes to 900.",
          "Changed `APPROVAL_LIMIT_CENTS` to 100,000 for EUR and 90,000 for GBP, and the tests that refund a 700 EUR order now expect it to go straight through.",
          tools=[("read", "app/payments/refunds.py"),
                 ("edit", "app/payments/refunds.py", "APPROVAL_LIMIT_CENTS = {\"EUR\": 50_000, \"GBP\": 45_000}\n", "APPROVAL_LIMIT_CENTS = {\"EUR\": 100_000, \"GBP\": 90_000}\n"),
                 tests(26, "4.4")],
          notes=[N("decision", "Refunds over 1000 EUR or 900 GBP need a second approver from the finance group; finance raised the limit from 500 EUR on 2 Nov.", ["refunds", "approval", "finance"], replaces="Refunds over 500 EUR or 450 GBP")]),
        T("commit", "Committed as \"Approval limit 1000 EUR / 900 GBP\".", tools=[("commit", "Approval limit 1000 EUR / 900 GBP")], gap=1),
    ]},
    {"when": "2026-11-03 14:15", "turns": [
        T("CI takes 9 minutes. Can we make it faster?",
          "Most of it is installing: uv downloads every package on each run, and the tests run one at a time. I turned on setup-uv's cache and run pytest with `-n auto` (pytest-xdist), which is a new dev dependency, so I left it out until you say: with the cache alone it is about 6 minutes, with xdist about 3.",
          tools=[("read", ".github/workflows/ci.yml"),
                 ("edit", ".github/workflows/ci.yml", "      - uses: astral-sh/setup-uv@v3\n", "      - uses: astral-sh/setup-uv@v3\n        with:\n          enable-cache: true\n")]),
        T("Add xdist. And while you're there: mypy --strict on app/payments has to be a required check.",
          "Added pytest-xdist to the dev group, `-n auto` in CI, and a `mypy --strict app/payments` step that fails the job. CI took 3 min 05 s on this branch.",
          tools=[("bash", "uv add --dev pytest-xdist", "Resolved 43 packages in 298ms\n + execnet==2.1.1\n + pytest-xdist==3.6.1", 0),
                 ("edit", ".github/workflows/ci.yml", "      - run: uv run pytest -q\n", "      - run: uv run mypy --strict app/payments\n      - run: uv run pytest -q -n auto\n"),
                 ("bash", "uv run mypy --strict app/payments", "Success: no issues found in 4 source files", 0)],
          notes=[N("decision", "CI caches uv's downloads and runs pytest -n auto (pytest-xdist): about 3 minutes instead of 9.", ["CI", "pytest-xdist"]),
                 N("rule", "mypy --strict must pass on app/payments: it is a required CI check.", ["mypy", "CI", "app/payments"], when=["changing payments code"])]),
        T("commit", "Committed as \"Faster CI, strict types on payments\".", tools=[("commit", "Faster CI, strict types on payments")], gap=1),
    ]},
    {"when": "2026-11-04 08:20", "turns": [
        T("We refunded order 88412 twice last night. Stripe shows two refunds 40 seconds apart. What happened?",
          "The support tool timed out on the first request (Stripe was slow, 31 s), retried, and sent a new Idempotency-Key with the retry, so Stripe saw two different refunds. Our side refunded twice because nothing remembers which keys were used for which order: the order's status check passed both times because the first request had not committed yet. I added `refund_requests` (migration 0010) with a unique (order_id, idempotency_key), written before Stripe is called, and a per-order lock so that two refunds of one order cannot run at once.",
          tools=[("bash", "psql $STAGING_RO -c \"select id, order_id, created_at from refunds where order_id = 88412\"", " id   | order_id |       created_at\n------+----------+------------------------\n 7710 |    88412 | 2026-11-03 23:41:07+00\n 7711 |    88412 | 2026-11-03 23:41:47+00\n(2 rows)", 0),
                 ("bash", "kubectl -n prod logs deploy/support-tool --since=12h | grep 88412", "23:41:07 refund order=88412 key=sup-5f1c timeout after 30s, retrying\n23:41:47 refund order=88412 key=sup-a93e ok", 0),
                 ("read", "app/payments/refunds.py"),
                 ("write", "migrations/0010_refund_requests.sql", "CREATE TABLE refund_requests (\n    id BIGSERIAL PRIMARY KEY,\n    order_id BIGINT NOT NULL REFERENCES orders(id),\n    idempotency_key TEXT NOT NULL,\n    refund_id TEXT,\n    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),\n    UNIQUE (order_id, idempotency_key)\n);\n"),
                 ("edit", "app/payments/refunds.py", "    def refund(self, order_id: int, idempotency_key: str) -> str:\n        order = self.db.get(Order, order_id)\n", "    def refund(self, order_id: int, idempotency_key: str) -> str:\n        # One refund of an order at a time, and a key used once per order.\n        order = self.db.get(Order, order_id, with_for_update=True)\n        if (done := self._earlier(order_id, idempotency_key)) is not None:\n            return done\n"),
                 tests(29, "5.0")],
          notes=[N("fact", "Incident 3 Nov: order 88412 was refunded twice because the support tool retried a timed-out refund with a new idempotency key.", ["incident", "refunds", "support tool"]),
                 N("decision", "Each refund request is recorded in refund_requests with a unique (order_id, idempotency_key) before Stripe is called, under a row lock on the order; a repeat returns the first refund.", ["refunds", "idempotency", "refund_requests"]),
                 N("requirement", "The support tool must reuse the same idempotency key when it retries a refund (support team's code; they have the ticket).", ["support tool", "idempotency"])]),
        T("commit", "Committed as \"One refund per order and key\".", tools=[("commit", "One refund per order and key")], gap=1),
    ]},
    {"when": "2026-11-05 13:40", "turns": [
        T("Product wants partial refunds behind a feature flag. Should we use LaunchDarkly?",
          "It would work, but for a handful of flags in one service it is a lot: a client library, a new vendor to review, and the cost per seat. An environment variable per flag, read once at start, covers what you described. Your call.",
          tools=[("grep", "FLAG_|feature", "app", "")]),
        T("No LaunchDarkly, too expensive for what we need. Do env flags.",
          "Added `app/flags.py`: `enabled(\"partial_refunds\")` reads `FLAG_PARTIAL_REFUNDS=1` once, at start. No new dependency.",
          tools=[("write", "app/flags.py", "\"\"\"Feature flags: FLAG_<NAME>=1 in the environment, read once at start.\"\"\"\nimport os\nfrom functools import cache\n\n\n@cache\ndef enabled(name: str) -> bool:\n    return os.environ.get(f\"FLAG_{name.upper()}\", \"\") == \"1\"\n")],
          notes=[N("decision", "Feature flags are environment variables read once at start by app/flags.py (FLAG_<NAME>=1); LaunchDarkly was turned down on cost.", ["feature flags", "LaunchDarkly"])]),
    ]},
    {"when": "2026-11-06 10:10", "turns": [
        T("Let's do partial refunds now, behind FLAG_PARTIAL_REFUNDS.",
          "With the flag on, the refund request takes an optional `amount_cents`: at most what is left to refund on the order (its total minus earlier refunds), else 409. The approval limit counts all of an order's refunds together, so two 600 EUR refunds of one order need approval for the second. Without the flag the endpoint refunds the whole order as before.",
          tools=[("read", "app/payments/refunds.py"), ("read", "app/flags.py"),
                 ("edit", "app/payments/refunds.py", "    def refund(self, order_id: int, idempotency_key: str) -> str:\n", "    def refund(self, order_id: int, idempotency_key: str, amount_cents: int | None = None) -> str:\n"),
                 ("edit", "app/payments/refunds.py", "        if order.total_cents > APPROVAL_LIMIT_CENTS[order.currency]:\n", "        left = order.total_cents - self._refunded(order_id)\n        amount = left if amount_cents is None or not enabled(\"partial_refunds\") else amount_cents\n        if not 0 < amount <= left:\n            raise RefundError(f\"at most {left} cents are left to refund\")\n        if self._refunded(order_id) + amount > APPROVAL_LIMIT_CENTS[order.currency]:\n"),
                 ("edit", "app/payments/refunds.py", "from app.orders.models import Order\n", "from app.flags import enabled\nfrom app.orders.models import Order\n"),
                 tests(33, "5.2")],
          notes=[N("decision", "Partial refunds (FLAG_PARTIAL_REFUNDS): the amount is at most what is left to refund on the order, and the approval limit counts all of an order's refunds together.", ["partial refunds", "refunds", "approval"]),
                 DONE("Partial refunds: POST /orders/{id}/refunds")]),
        T("commit", "Committed as \"Partial refunds behind a flag\".", tools=[("commit", "Partial refunds behind a flag")], gap=1),
    ]},
    {"when": "2026-11-09 09:05", "turns": [
        T("Finance says the daily settlement report is off by one day for orders placed after 11 pm. Look at it.",
          "The report cut days at midnight UTC, and finance counts days in Berlin: an order at 23:30 Berlin time (22:30 UTC in winter) is the same day for both, but one at 00:30 Berlin is still the previous day in UTC. I made the report take Europe/Berlin days and convert their bounds to UTC; timestamps stay UTC in the database.",
          tools=[("read", "app/reports/settlement.py"),
                 ("edit", "app/reports/settlement.py", "from datetime import date, datetime, time\n", "from datetime import date, datetime, time\nfrom zoneinfo import ZoneInfo\n"),
                 ("edit", "app/reports/settlement.py", "    start = datetime.combine(day, time.min)\n    end = datetime.combine(day, time.max)\n", "    # Finance's days are Berlin days; the database keeps UTC.\n    berlin = ZoneInfo(\"Europe/Berlin\")\n    start = datetime.combine(day, time.min, berlin)\n    end = datetime.combine(day, time.max, berlin)\n"),
                 tests(34, "5.1")],
          notes=[N("decision", "The settlement report counts Europe/Berlin days (finance's), not UTC days; timestamps stay UTC in the database.", ["settlement report", "timezone", "finance"])]),
        T("commit", "Committed as \"Settlement report in Berlin days\".", tools=[("commit", "Settlement report in Berlin days")], gap=1),
    ]},
    {"when": "2026-11-10 15:30", "turns": [
        T("How do we release? Nobody wrote it down. Put it in the runbook.",
          "I wrote down what the CI and the deploy scripts do: a release is a tag `vX.Y.Z` on main, which builds the image `harbor:vX.Y.Z`; production deploys a tag's image, never main's; the tag needs a CHANGELOG.md entry, which CI checks.",
          tools=[("read", "docs/runbook.md"), ("read", ".github/workflows/ci.yml"),
                 ("edit", "docs/runbook.md", "Merges to main build an image; staging takes it within ten minutes.\n", "Merges to main build an image; staging takes it within ten minutes.\n\n## Releases\n\n1. Add the version's entry to CHANGELOG.md on main.\n2. Tag main: `git tag v0.15.0 && git push origin v0.15.0`.\n3. CI builds `harbor:v0.15.0`; deploy it with `make deploy TAG=v0.15.0`.\n\nProduction runs tagged images only, never main.\n")],
          notes=[N("rule", "Releases are tags vX.Y.Z cut from main, each with a CHANGELOG.md entry; production deploys a tag's image, never main.", ["releases", "deploys"], when=["shipping to production"])]),
        T("Also add: no production deploys on Fridays.",
          "Added to the runbook's release section.",
          tools=[("edit", "docs/runbook.md", "Production runs tagged images only, never main.\n", "Production runs tagged images only, never main. No production deploys on Fridays.\n")],
          notes=[N("rule", "No production deploys on Fridays.", ["deploys"], when=["shipping to production"])]),
        T("commit", "Committed as \"Write the release process down\".", tools=[("commit", "Write the release process down")], gap=1),
    ]},
    {"when": "2026-11-11 11:15", "turns": [
        T("Northwind, the partner, is hitting 429s all day. Give partner keys 300 a minute.",
          "Partner keys (api_keys.tier = 'partner') now get 300 requests a minute; everyone else keeps 100, and the burst stays 20. I added a print of the tier while testing.",
          tools=[("read", "app/rate_limit.py"),
                 ("edit", "app/rate_limit.py", "RATE_PER_MINUTE = 100\n", "RATE_PER_MINUTE = {\"standard\": 100, \"partner\": 300}\n"),
                 ("edit", "app/rate_limit.py", "    ok, wait = allow(request.headers.get(\"x-api-key\", request.client.host))\n", "    key = request.headers.get(\"x-api-key\", request.client.host)\n    tier = tier_of(key)\n    print(\"rate limit tier\", key, tier)\n    ok, wait = allow(key, RATE_PER_MINUTE[tier])\n"),
                 tests(35, "5.3")],
          notes=[N("decision", "Rate limits: 100 requests a minute per API key, 300 for partner keys (api_keys.tier = 'partner'), burst 20; 429 with Retry-After over it.", ["rate limiting", "partners", "API keys"], replaces="Rate limiting is a token bucket per API key")]),
        T("You left a print() in rate_limit.py, and it prints the API key. We use log.debug and we never log keys, it's in the rules.",
          "Sorry, removed. It logs `log.debug(\"rate limit\", tier=tier)` now, with no key.",
          tools=[("edit", "app/rate_limit.py", "    print(\"rate limit tier\", key, tier)\n", "    log.debug(\"rate limit\", tier=tier)\n"),
                 ("edit", "app/rate_limit.py", "from app.config import settings\n", "import structlog\n\nfrom app.config import settings\n\nlog = structlog.get_logger()\n")],
          notes=[N("rule", "Never log API keys or secrets, even at debug level.", ["logging", "API keys"], when=["adding a log line"])]),
        T("commit", "Committed as \"300 requests a minute for partners\".", tools=[("commit", "300 requests a minute for partners")], gap=1),
    ]},
    {"when": "2026-11-12 16:00", "turns": [
        T("What's still open on payments before the UK launch on December 1st?",
          "From what we noted: the support tool still sends a new idempotency key when it retries a refund (the support team's fix; our side refuses duplicates since 4 Nov, but they would see errors); the settlement report groups by currency but finance has not said whether GBP days are London days; and nothing yet sends GBP refunds through the approval flow end to end in staging.",
          notes=[N("requirement", "Before the UK launch (1 Dec): ask finance whether the GBP settlement report counts London or Berlin days.", ["UK launch", "settlement report", "GBP"]),
                 N("requirement", "Before the UK launch (1 Dec): run a GBP refund over 900 GBP through approval end to end in staging.", ["UK launch", "refunds", "GBP"])]),
        T("Dana Okafor in finance is the approver of record for refunds over the limit, put that in.",
          "Noted: Dana Okafor (finance) is the approver of record for refunds over the limit.",
          notes=[N("fact", "Dana Okafor in finance is the approver of record for refunds over the approval limit.", ["Dana Okafor", "finance", "approval"])]),
    ]},
    {"when": "2026-11-13 09:40", "turns": [
        T("Staging: a GBP refund webhook comes back as an error. Check the charge.refunded handler with a GBP order.",
          "The webhook handler did nothing with `charge.refunded` events, so it did not fail, but the refund stayed `pending` in our table: I made it mark the refund settled by its Stripe id, and it reads the amount in the event's currency, not EUR. A GBP refund from staging's Stripe now settles.",
          tools=[("read", "app/webhooks.py"),
                 ("bash", "stripe trigger charge.refunded --override charge:currency=gbp", "Setting up fixture for: charge\nRunning fixture for: charge\nSetting up fixture for: refund\nTrigger succeeded! Check dashboard for event details.", 0),
                 ("edit", "app/webhooks.py", "        pass  # mark the refund as settled\n", "        refund = event[\"data\"][\"object\"][\"refunds\"][\"data\"][0]\n        RefundService.settle(refund[\"id\"], refund[\"amount\"], refund[\"currency\"].upper())\n"),
                 ("edit", "app/webhooks.py", "from app.payments.client import PaymentsClient\n", "from app.payments.client import PaymentsClient\nfrom app.payments.refunds import RefundService\n"),
                 tests(36, "5.4")]),
        T("commit and that's it for today", "Committed as \"Settle refunds from the webhook\".", tools=[("commit", "Settle refunds from the webhook")], gap=1),
    ]},
]

# Questions the demo asks the memory afterwards, with what should answer them.
QUESTIONS = [
    ("Who approves a refund of 1,200 EUR?", "1000 EUR"),
    ("Can I call stripe.Refund.create in a route?", "PaymentsClient"),
    ("How do I add a column to refund_approvals?", "forward-only"),
    ("What may we log?", "ids only"),
    ("Why did order 88412 get refunded twice?", "idempotency"),
]
