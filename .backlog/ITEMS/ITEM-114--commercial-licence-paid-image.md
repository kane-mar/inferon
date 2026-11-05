## ITEM-114: Commercial licence and paid image (the for-profit path)

**As** the owner of the company this project becomes
**I want** a paid path for organisations that cannot accept the AGPL
**So that** the free tier stays free for traction while the revenue comes from the
people who need different terms

**Context (2026-11-05, PO request).** The intent is a for-profit company selling both
the hosted service and a licence of the Docker image, with open source (ITEM-111)
used as the publicity mechanism first. This item is the second half of that plan and
is **deliberately gated on traction**: selling a licence before anyone wants one is
work on a customer that does not exist yet. It exists now so the ITEM-111 licence
choice is made with this end state in view, and so nobody "simplifies" the licence
later without seeing what that would cost.

**Trigger to unpark** (whichever comes first):
- the first inbound "can we pay you / we cannot use AGPL", or
- 10 active external workspaces, or
- a procurement conversation that stops on the licence.

**Acceptance Criteria (when unparked)**
1. A commercial licence exists for the same artifact as the AGPL release: no forked
   binary, no feature that only money can unlock in the free build. The paid tier buys
   **terms** (relief from AGPL obligations, warranty, indemnity, support SLA), not
   extra code.
2. One published comparison table: what the AGPL gives you for free, what the
   commercial licence adds, and how to tell which one you need.
3. Entitlement works offline: a signed licence file in the image's environment, no
   phone-home, no kill switch. An air-gapped install can be licensed.
4. An order and fulfilment path that does not need a sales team: self-serve checkout,
   licence file by email, invoice option for procurement.
5. Pricing is published, with the hosted service priced separately from the image
   licence.
6. The free and paid images are built from the same tag and published together, so a
   security fix reaches both on the same day.
7. `README.md` and the docs state plainly that self-hosting is free under the AGPL and
   that the commercial licence exists for organisations that cannot meet it.
8. The support boundary is written down for both tiers (what a licence buys, what it
   does not).

**Technical Notes**
- This item needs ITEM-111 finished: dual licensing is only possible because the CLA
  gives us the right to relicense. If ITEM-111 chose Apache-2.0 instead, this item
  changes shape entirely (the revenue would come from services and proprietary
  add-ons, not from a licence).
- Wrapping the paid path in a "may not compete" clause inside the AGPL build is the
  one move that would break both the open-source claim and this item's premise.
- The honest risk: licence-only revenue is small and slow. Expect services and the
  hosted tier to carry the company; the licence mostly exists so procurement can say
  yes.

**Source:** PO request, 2026-11-05 ("eventually be able to create a for-profit company
selling both the services and a license of the docker image").
