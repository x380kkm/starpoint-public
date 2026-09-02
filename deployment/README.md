# Deploying StarPoint

## DISCLAIMER

**While these scripts exist to facilitate ease of deployment, they are not a one-size-fits-all solution and may not work if not properly configured.**

**THESE FILES ARE PROVIDED FOR PERSONAL USE AND NOT DESIGNED FOR CREATING PUBLICLY-AVAILABLE SERVICES. CREATING AND SERVING A PUBLICLY-AVAILABLE SERVER IS HIGHLY DISCOURAGED, ESPECIALLY WITH THESE FILES AS PROVIDED. NO AUTHOR NOR CONTRIBUTOR TO STARPOINT ENDORSES NOR ASSUMES RESPONSIBILITY OF ANY KIND FOR ANY CONSEQUENCES OF ANY KIND SHOULD ONE ATTEMPT TO MAKE AVAILABLE ANY SERVICES USING THIS REPOSITORY. BY RUNNING ANY PART OF THIS CODE, AN INDIVIDUAL AGREES TO RELEASE STARPOINT AND ITS CONTRIBUTORS OF ALL LIABILITY. VOLUNTARY ASSISTANCE WITH LOCAL SETUPS SHALL NOT BE CONSIDERED LIABILITY, NOR ENDORSEMENT.**

This folder currently contains configuration files for:

- Nginx reverse proxy configuration
- SSL self-signed certificate and key generation script (for POSIX shell (Linux/Unix/Mac) and Windows) (requires installing OpenSSL)
  - Automatic certificate installation for Linux systems, to the paths the nginx file expects
- dnsmasq DNS redirection
- systemd service file
- Installation script
- Utilities shell script file, to be imported by other scripts for extra functions

Ensure you have the required dependencies for the scripts you want to run. Note that npm must be run on the target system to build dependencies. Running build tasks on another system and copying the output over may not work.

Make sure to change the host address in .env to something other than localhost!

## CN local deployment

Use `deployment/cn/run.ps1` on Windows or `deployment/cn/run.sh` on Linux. Each entry prepares the CN CDN, creates `.env.cn` with local management credentials when needed, builds the server, and starts the HTTP and TCP services in the foreground. Add `-ValidateOnly` or `--validate-only` to verify an existing CDN against the release manifest without downloading files, changing `.env.cn`, building, or starting a service. Add `-HealthCheck` or `--health-check` when checking an already-running local instance without starting another process.

After the service starts, use `deployment/cn/check.ps1` or `deployment/cn/check.sh` to query `/healthz`. The health response contains only the service status, HTTP port, multiplayer port, and server time. Use `/manage` for authenticated administration; the health endpoint does not replace management authentication.

CN multiplayer room expiry and COM fill delay use the configured virtual server time, so accelerated events keep matchmaking and battle timestamps on the same clock.

## Local personal service

Use `deployment/personal-service/run.ps1` on Windows or `deployment/personal-service/run.sh` on Linux when the Rust personal service must run as a standalone local process. The scripts build the `personal-service` binary, store data in `data/personal-service` by default, serve CN assets from `<root>/cdn/cn` by default, and bind only to IPv4 loopback. Pass `-CdnRoot PATH` or `--cdn-root PATH` to use a prepared external `.cdn/cn` directory, or set `STARPOINT_PERSONAL_SERVICE_CDN_ROOT`. Pass `-ShowManagementToken` or `--show-management-token` to print the in-memory management token for the local `/manage` page. Pass `-LogHttpAccess` or `--log-http-access` to write only the HTTP method, path without query parameters, response status, and response Content-Type to stderr. Type `stop` or `quit`, or send Ctrl-C/SIGTERM, to checkpoint and exit.

The management token can issue one player token per CN viewer through `POST /v1/player-access` and revoke it through `DELETE /v1/player-access/{viewer_id}`. A player token can list, import, export, encrypt-export, activate, and use the administrator-configured encrypted save targets only for its granted slots under `/v1/player/local-saves`; it cannot call administrator endpoints.

Open `/player` for the ordinary player page. It accepts the issued player token in memory and provides only the granted save slots, JSON or encrypted export, import, refresh, and device activation.
