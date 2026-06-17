# TLS certificate renewal (production)

Production serves HTTPS for `trove.chultarsky.me` with a Let's Encrypt
certificate. This document is the source of truth for **how renewal works**,
**how it failed once**, and **how to recover** — because the live renewal
configuration lives on the host, not in this repo, and silently drifted out
of sync once (see the incident below).

## How it's wired

- The `proxy` service (`nginx:alpine`, see `docker-compose.prod.yml`) owns
  ports **80 and 443** and terminates TLS. It mounts the host's
  `/etc/letsencrypt` (read-only) and `/var/www/certbot` (read-only).
- **Certbot runs on the host** (Ubuntu, `apt` package), *not* in the compose
  stack. Renewal uses the **webroot** method: certbot writes the ACME
  HTTP-01 challenge to `/var/www/certbot/.well-known/acme-challenge/`, and
  nginx serves it on port 80 (`location /.well-known/acme-challenge/` in
  `nginx.prod.conf`). Everything else on port 80 is 301-redirected to HTTPS.
- `certbot.timer` (systemd) runs `certbot renew` twice daily. On a successful
  renewal, the deploy hook **`reload-nginx.sh`** reloads nginx inside the
  proxy container so it picks up the new cert (the cert dir is mounted
  read-only and nginx caches certs in memory — a renewal without a reload
  leaves nginx serving the old cert).

> ⚠️ **The webroot method is mandatory here.** Do **not** use `--standalone`:
> it spins up certbot's own server on port 80, which nginx already owns, so
> the renewal fails with `Could not bind TCP port 80`. This is exactly the
> bug that caused the June 2026 outage.

## Files that live on the host (keep them correct)

| Host path | What it must contain |
|---|---|
| `/etc/letsencrypt/renewal/trove.chultarsky.me.conf` | `authenticator = webroot` and `webroot_path = /var/www/certbot` |
| `/etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh` | The reload hook — version-controlled copy in [`renewal-hooks/reload-nginx.sh`](renewal-hooks/reload-nginx.sh). **Only `reload-nginx.sh` belongs in this dir** — certbot executes *every* file there, so never leave `.bak` copies in it. |

## The June 2026 incident (postmortem)

- **Symptom:** the site became unreachable — browsers and `curl` rejected the
  TLS cert as expired. The cert had expired at its 90-day mark; auto-renewal
  had silently failed for ~30 days.
- **Root cause:** the host's renewal config had `authenticator = standalone`
  (a leftover from initial issuance, before nginx ran on port 80). The
  `certbot.timer` fired faithfully twice a day, but every run died with
  `Could not bind TCP port 80 because it is already in use` — nginx owns it.
  Standalone and the nginx-webroot architecture are mutually exclusive.
- **Why it was silent:** failed renewals don't page anyone, and the original
  `reload-nginx.sh` had its own bugs (ran `docker compose` without the env
  file; grepped `ps` output for `running` which never matched compose's
  `Up …`) — but those never even executed, because renewal failed *before*
  reaching the deploy-hook stage.
- **Fix:** switched the renewal config to `webroot` (`-w /var/www/certbot`),
  reissued the cert, replaced `reload-nginx.sh` with a version that talks to
  Docker directly (no compose/env dependency, reliable running-check), and
  proved the whole path with `certbot renew --dry-run --run-deploy-hooks`.

## Recovery runbook

If the cert is expiring/expired or renewal is failing:

```bash
ssh -i ~/.ssh/lightsail-ember-trove.pem ubuntu@18.221.254.95

# 1. Diagnose
sudo certbot certificates                       # expiry + INVALID/VALID
systemctl is-enabled certbot.timer; systemctl is-active certbot.timer
grep authenticator /etc/letsencrypt/renewal/trove.chultarsky.me.conf  # must be 'webroot'
sudo tail -n 40 /var/log/letsencrypt/letsencrypt.log

# 2. Renew now via webroot (works WITH nginx running) and (re)set the method
sudo certbot certonly --webroot -w /var/www/certbot -d trove.chultarsky.me --non-interactive

# 3. Reload nginx to serve the new cert (the deploy hook does this on auto-renew)
docker exec deploy-proxy-1 nginx -s reload

# 4. Prove auto-renewal end to end
sudo certbot renew --dry-run --run-deploy-hooks    # expect "all simulated renewals succeeded"
```

Verify from outside:

```bash
curl -sS -o /dev/null -w "%{http_code} ssl_verify_result=%{ssl_verify_result}\n" \
  https://trove.chultarsky.me/api/health     # expect: 200 ssl_verify_result=0
```
