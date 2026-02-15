# Zela Feedback

- What was confusing/missing:
  - Public examples for a no-input `CustomProcedure` that uses `RpcClient::get_leader_schedule`.
  - Clear statement of preferred crate target layout (`lib`/`cdylib`) for interview submissions.
  - Zela has examples on how to auth and deploy for the `hello_world` example crate. This can be configured to be available for all custom procedures.
- Improvement ideas:
  - Add an official template repo with:
    - minimal `CustomProcedure` scaffold
    - CI check for procedure exports
    - one end-to-end executor call example
