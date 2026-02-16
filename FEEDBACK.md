# Zela Feedback

## What I've loved
Awesome idea and great features at this point! This product will help all developers automate their interaction with the blockchain. Zela has a lot of use cases and would love to help you guys with it! It was really easy to connect to the GitHub repo, it was basically plug and play. Procedure creation was really easy as well. Logging is easy to read and access. It has a lot of potential.

## Possible Improvements
### Dashboard
- Dashboard should not show [0 calls.](./assets/zela-0-calls.png) This makes it seem like there is downtime. 
- Procedure versioning will help with the DX (development/release procedures, reverts etc).
- Hover information for the Dashboard: Procs ready, Build Errors, Exec errors, RPC calls. Small statistic inside the dashboard will help developers get quick feedback from Zela on how their work is doing.
- Add execution logs in the main dashboard for all procedures. This will make the dashboard retain the developer and thus reduces the number of clicks. Each procedure can have separate "in-depth" details.
- In the Procedure Details page add a `Copy` button for the build commit.
- Add an automation rotation of the keys: once a week/month/year etc.
### Code and Docs
- Add docs in the website directly. Official deployment to `crate.io` and more code docs will also help.
- Public examples for a no-input `CustomProcedure` that uses `RpcClient::get_leader_schedule`.
- At this point there are bash scripts examples on how to auth and deploy the `hello_world` crate. User defined procedures should also have some boiler plate bash scripts to make the auth and execution steps easier.
- It was pretty hard to determine the input parameters for the scripts without a lot of trial and error. It is pretty confusing that you have to put `zela.{procedure}{hash}`. I think it would be nice to also include the project there: `zela.{project}{procedure}{hash}`.
- Add an official template repo with:
  - minimal `CustomProcedure` scaffold
  - CI check for procedure exports: this can be extended to CI/CD action workflow
  - one end-to-end executor call example
