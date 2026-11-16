#!/usr/bin/env python3
"""Check the public nested example's six decisions and ordered staged effects."""
import difflib
from pathlib import Path
import subprocess
import sys


EXPECTED = """source=Open(Draft) event=Submit
Accepted revision=1 transition=1
state=Open(AwaitingPayment)
intent=Exit(Draft) (not dispatched)
intent=RequestPayment (not dispatched)
intent=Enter(AwaitingPayment) (not dispatched)
source=Open(AwaitingPayment) event=PaymentConfirmed(0)
Rejected guard=1
state=Open(AwaitingPayment)
source=Open(AwaitingPayment) event=PaymentConfirmed(7)
Accepted revision=2 transition=2
state=Paid { receipt: 7 }
intent=Exit(AwaitingPayment) (not dispatched)
intent=Exit(Open) (not dispatched)
intent=Enter(Paid) (not dispatched)
source=Paid { receipt: 7 } event=RefreshReceipt
Accepted revision=3 transition=4
state=Paid { receipt: 7 }
intent=Exit(Paid) (not dispatched)
intent=Enter(Paid) (not dispatched)
source=Paid { receipt: 7 } event=Cancel
Rejected guard=2
state=Paid { receipt: 7 }
source=Open(Draft) event=Cancel
Accepted revision=1 transition=3
state=Cancelled
intent=Exit(Draft) (not dispatched)
intent=Exit(Open) (not dispatched)
intent=NotifyCancellation (not dispatched)
intent=Enter(Cancelled) (not dispatched)
"""


def main():
    result = subprocess.run(
        ["cargo", "run", "--manifest-path", "ci/machine-consumer/Cargo.toml",
         "--bin", "nested", "--locked"],
        cwd=Path(__file__).resolve().parents[1],
        stdout=subprocess.PIPE,
        text=True,
        check=False,
    )
    print(result.stdout, end="")
    if result.returncode:
        return result.returncode
    if result.stdout != EXPECTED:
        sys.stderr.writelines(difflib.unified_diff(
            EXPECTED.splitlines(keepends=True), result.stdout.splitlines(keepends=True),
            fromfile="expected nested trace", tofile="actual nested trace",
        ))
        return 1
    print("Nested example: six decisions and exact staged traces verified")
    return 0


if __name__ == "__main__":
    sys.exit(main())
