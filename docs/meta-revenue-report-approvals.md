# Meta-approved revenue report relaying

An issuer can configure an offering-level delegate with `set_meta_delegate`. The
delegate can then sign a `MetaAction::ApproveRevenueReport` authorization for a
specific offering and period. `meta_approve_revenue_report` verifies the
delegate signature and stores the complete signed payload, including the
approved payout asset and amount, together with the authorization expiry.

Anyone can relay an approved report by calling
`submit_approved_revenue_report(issuer, namespace, token, period_id)`. The
contract uses the stored payload without accepting replacement report values
and invokes the same internal report path as `report_revenue`. An approval is
bound to its offering and period, is rejected after its expiry, and is removed
after successful submission. A failed report does not consume the approval.

This relay entrypoint does not change the issuer-auth requirement for
`report_revenue` or `report_revenue_with_attestation`.
