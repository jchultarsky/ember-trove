# Deploying Ember Trove on AWS

This guide walks through deploying Ember Trove on **AWS Lightsail** with **Amazon Cognito** for authentication and **Lightsail Object Storage** for file attachments. It assumes you are starting from a fresh AWS account with a domain you control.

**Estimated monthly cost:** ~$21/mo (Lightsail $20 + Object Storage ~$1)

---

## Architecture

```
Browser ──HTTPS──► nginx (Lightsail VM, port 443)
                       ├── /api/*  ──► api container (port 3003)
                       └── /*      ──► ui container  (port 80)

api ──► PostgreSQL (Docker volume on same VM)
api ──► Lightsail Object Storage (S3-compatible, attachments)
api ──► Amazon Cognito (OIDC: login / token exchange / user management)
```

All four services (proxy, api, ui, postgres) run as Docker containers on a single Lightsail instance managed by `docker-compose.prod.yml`.

---

## Prerequisites

| Tool | Install |
|------|---------|
| AWS CLI v2 | [docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html) |
| SSH client | Built into macOS/Linux; OpenSSH on Windows |
| A registered domain | Any registrar; DNS managed in Route 53 or elsewhere |

Configure the AWS CLI with credentials that have sufficient permissions (or use an account with AdministratorAccess for initial setup):

```bash
aws configure
# AWS Access Key ID: <your key>
# AWS Secret Access Key: <your secret>
# Default region name: us-east-2
# Default output format: json
```

---

## Step 1 — Create a Lightsail Instance

1. Open **AWS Lightsail** → **Instances** → **Create instance**.
2. Select:
   - **Platform:** Linux/Unix
   - **Blueprint:** OS Only → **Ubuntu 22.04 LTS**
   - **Instance plan:** $20/mo (4 GB RAM, 2 vCPU, 80 GB SSD) — Rust compilation needs ≥ 4 GB
   - **Availability zone:** your preferred zone (e.g. `us-east-2a`)
3. Under **SSH key pair**, download (or select) a key pair. Note the path — you'll use it for every SSH command.
4. Name the instance (e.g. `ember-trove`) and click **Create instance**.

Wait ~60 seconds for the instance to reach **Running** state.

### Attach a static IP

Without a static IP, the public IP changes on every reboot.

1. In Lightsail → **Networking** → **Create static IP**.
2. Attach it to your new instance.
3. Note the static IP address (e.g. `18.221.254.95`).

### Open firewall ports

In the instance's **Networking** tab → **IPv4 Firewall**, ensure these rules exist:

| Protocol | Port | Purpose |
|----------|------|---------|
| TCP | 22 | SSH |
| TCP | 80 | HTTP (certbot + redirect to HTTPS) |
| TCP | 443 | HTTPS |

---

## Step 2 — Point DNS at the Instance

Add an **A record** in your DNS provider pointing your subdomain to the Lightsail static IP:

| Name | Type | Value |
|------|------|-------|
| `trove` | A | `<your static IP>` |

(If your DNS is in Route 53: Hosted Zones → your domain → Create record.)

DNS propagation takes 1–60 minutes. Verify with:

```bash
dig +short trove.yourdomain.com
# should return your static IP
```

---

## Step 3 — Set Up Amazon Cognito

Cognito replaces Keycloak. You get a fully managed, OIDC-compatible identity provider for free (up to 50 000 MAU).

### 3a. Create a User Pool

```bash
aws cognito-idp create-user-pool \
  --pool-name ember-trove \
  --region us-east-2 \
  --auto-verified-attributes email \
  --username-attributes email \
  --schema '[{"Name":"email","Required":true,"Mutable":true}]' \
  --policies '{
    "PasswordPolicy": {
      "MinimumLength": 8,
      "RequireUppercase": true,
      "RequireLowercase": true,
      "RequireNumbers": true,
      "RequireSymbols": false,
      "TemporaryPasswordValidityDays": 7
    }
  }' \
  --mfa-configuration "OFF"
```

Note the `Id` field from the response (e.g. `us-east-2_XXXXXXXXX`). This is your **User Pool ID**.

### 3b. Create an App Client

```bash
aws cognito-idp create-user-pool-client \
  --user-pool-id us-east-2_XXXXXXXXX \
  --client-name ember-trove-api \
  --region us-east-2 \
  --generate-secret \
  --explicit-auth-flows ALLOW_REFRESH_TOKEN_AUTH ALLOW_USER_PASSWORD_AUTH \
  --allowed-o-auth-flows code \
  --allowed-o-auth-scopes openid email profile \
  --allowed-o-auth-flows-user-pool-client \
  --callback-urls '["https://trove.yourdomain.com/api/auth/callback"]' \
  --logout-urls '["https://trove.yourdomain.com"]' \
  --supported-identity-providers COGNITO \
  --token-validity-units '{"AccessToken":"hours","IdToken":"hours","RefreshToken":"days"}' \
  --access-token-validity 1 \
  --id-token-validity 1 \
  --refresh-token-validity 30
```

Note the `ClientId` and `ClientSecret` from the response.

### 3c. Set Up the Hosted UI domain

Cognito needs a domain name to host its login UI:

```bash
aws cognito-idp create-user-pool-domain \
  --user-pool-id us-east-2_XXXXXXXXX \
  --domain trove-yourdomain \
  --region us-east-2
```

This creates `trove-yourdomain.auth.us-east-2.amazoncognito.com`.

### 3d. Create an admin user

```bash
aws cognito-idp admin-create-user \
  --user-pool-id us-east-2_XXXXXXXXX \
  --username you@yourdomain.com \
  --user-attributes Name=email,Value=you@yourdomain.com Name=email_verified,Value=true \
  --message-action SUPPRESS \
  --region us-east-2

aws cognito-idp admin-set-user-password \
  --user-pool-id us-east-2_XXXXXXXXX \
  --username you@yourdomain.com \
  --password 'YourPassword1!' \
  --permanent \
  --region us-east-2
```

### 3e. Assign the admin role

Ember Trove reads a `custom:roles` attribute to determine access. Add it to the user:

```bash
aws cognito-idp admin-update-user-attributes \
  --user-pool-id us-east-2_XXXXXXXXX \
  --username you@yourdomain.com \
  --user-attributes Name=custom:roles,Value=admin \
  --region us-east-2
```

> **Note:** You must also add `custom:roles` as a custom attribute in the User Pool **before** assigning it. In the AWS console: User Pool → **Attributes** → Add custom attribute → name `roles`, type String.

### 3f. Note the OIDC Issuer URL

The issuer follows the pattern:

```
https://cognito-idp.<region>.amazonaws.com/<user-pool-id>
```

Example: `https://cognito-idp.us-east-2.amazonaws.com/us-east-2_XXXXXXXXX`

---

## Step 4 — Create Lightsail Object Storage

This provides an S3-compatible bucket for file attachments at ~$1/mo.

1. Lightsail console → **Storage** → **Create bucket**.
2. Choose the **$1/mo plan** (5 GB), name the bucket (e.g. `ember-trove-files`), select your region.
3. After creation, go to **Permissions** → **Access keys** → **Create access key**.
4. Save the **Access key ID** and **Secret access key** — they are shown only once.

> **S3 endpoint for Lightsail Object Storage:**
> `https://s3.<region>.amazonaws.com` — it is standard AWS S3, no custom endpoint required.

---

## Step 5 — Create an IAM User for Cognito Admin

The API uses the AWS SDK to manage Cognito users (create/delete/list). Create a least-privilege IAM user for this:

1. IAM console → **Users** → **Create user**, name it `ember-trove-api`.
2. Attach the policy below inline:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "cognito-idp:ListUsers",
        "cognito-idp:AdminCreateUser",
        "cognito-idp:AdminDeleteUser",
        "cognito-idp:AdminSetUserPassword",
        "cognito-idp:AdminUpdateUserAttributes",
        "cognito-idp:ListGroups",
        "cognito-idp:AdminAddUserToGroup",
        "cognito-idp:AdminRemoveUserFromGroup",
        "cognito-idp:AdminListGroupsForUser"
      ],
      "Resource": "arn:aws:cognito-idp:<region>:<account-id>:userpool/<user-pool-id>"
    }
  ]
}
```

3. **Security credentials** tab → **Create access key** → choose "Application running outside AWS".
4. Save the key ID and secret.

---

## Step 6 — Prepare the Lightsail Server

SSH into the instance:

```bash
ssh -i /path/to/your-key.pem ubuntu@<static-ip>
```

### Install Docker

```bash
sudo apt-get update
sudo apt-get install -y ca-certificates curl gnupg
sudo install -m 0755 -d /etc/apt/keyrings
curl -fsSL https://download.docker.com/linux/ubuntu/gpg \
  | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg
echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] \
  https://download.docker.com/linux/ubuntu $(. /etc/os-release && echo "$VERSION_CODENAME") stable" \
  | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
sudo apt-get update
sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin
sudo usermod -aG docker ubuntu
newgrp docker
```

Verify:

```bash
docker --version
docker compose version
```

### Install Certbot

```bash
sudo apt-get install -y certbot
```

---

## Step 7 — Clone the Repository

```bash
git clone https://github.com/jchultarsky/ember-trove.git
cd ember-trove
```

---

## Step 8 — Configure the Environment File

Copy the template and fill in your values:

```bash
cp deploy/.env.prod.template deploy/.env.prod
chmod 600 deploy/.env.prod
nano deploy/.env.prod
```

Fill in every `CHANGE_ME` value:

```env
# Database — generate with: openssl rand -hex 16
POSTGRES_PASSWORD=<random-16-byte-hex>

# Lightsail Object Storage (from Step 4)
S3_BUCKET=ember-trove-files
S3_ACCESS_KEY=<bucket-access-key-id>
S3_SECRET_KEY=<bucket-secret-access-key>

# Cognito OIDC (App Client secret from Step 3b)
OIDC_CLIENT_SECRET=<app-client-secret>

# Cognito Admin IAM user (from Step 5)
AWS_ACCESS_KEY_ID=<iam-access-key-id>
AWS_SECRET_ACCESS_KEY=<iam-secret-access-key>

# Cookie encryption — generate with: openssl rand -hex 64
COOKIE_KEY=<128-hex-chars>
```

---

## Step 9 — Adapt the Compose and nginx Files for Your Domain

The production files ship with `trove.chultarsky.me` hard-coded. Replace it with your own domain:

```bash
# In docker-compose.prod.yml — update FRONTEND_URL and API_EXTERNAL_URL:
sed -i 's|trove\.chultarsky\.me|trove.yourdomain.com|g' deploy/docker-compose.prod.yml

# In docker-compose.prod.yml — update OIDC_ISSUER with your User Pool ID:
sed -i 's|us-east-2_4RQfxhKqn|us-east-2_XXXXXXXXX|g' deploy/docker-compose.prod.yml

# In docker-compose.prod.yml — update OIDC_CLIENT_ID with your App Client ID:
sed -i 's|eogq2sehdad3uc8nmar7aneol|<your-app-client-id>|g' deploy/docker-compose.prod.yml

# In docker-compose.prod.yml — update COGNITO_USER_POOL_ID:
sed -i 's|COGNITO_USER_POOL_ID: us-east-2_4RQfxhKqn|COGNITO_USER_POOL_ID: us-east-2_XXXXXXXXX|g' deploy/docker-compose.prod.yml

# In nginx.prod.conf — update the server_name and certificate paths:
sed -i 's|trove\.chultarsky\.me|trove.yourdomain.com|g' deploy/nginx.prod.conf
```

---

## Step 10 — Obtain a TLS Certificate

Stop anything running on port 80, then use Certbot in standalone mode to issue the certificate:

```bash
sudo certbot certonly --standalone \
  -d trove.yourdomain.com \
  --email you@yourdomain.com \
  --agree-tos \
  --non-interactive
```

Certificates are written to `/etc/letsencrypt/live/trove.yourdomain.com/`.

---

## Step 11 — Start the Stack

From `~/ember-trove`:

```bash
docker compose -f deploy/docker-compose.prod.yml --env-file deploy/.env.prod build
docker compose -f deploy/docker-compose.prod.yml --env-file deploy/.env.prod up -d
```

The first build takes 30–45 minutes (full Rust compilation from scratch). Subsequent builds are fast thanks to Docker BuildKit layer caching.

Check all services are healthy:

```bash
docker compose -f deploy/docker-compose.prod.yml --env-file deploy/.env.prod ps
```

Expected output:

```
NAME                IMAGE           STATUS
deploy-api-1        deploy-api      Up X seconds
deploy-postgres-1   postgres:16-...  Up X minutes (healthy)
deploy-proxy-1      nginx:alpine    Up X seconds
deploy-ui-1         deploy-ui       Up X seconds
```

Verify over HTTPS:

```bash
curl https://trove.yourdomain.com/api/health
# {"status":"ok","service":"ember-trove-api","database":"ok"}
```

Open `https://trove.yourdomain.com` in a browser. You will be redirected to the Cognito hosted login page.

---

## Step 12 — Configure Certificate Auto-Renewal

Let's Encrypt certificates expire after 90 days. Set up automatic renewal:

### Create the Certbot deploy hook

The hook tells nginx to reload after a renewed certificate is written:

```bash
sudo mkdir -p /etc/letsencrypt/renewal-hooks/deploy
sudo tee /etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh > /dev/null <<'HOOK'
#!/bin/bash
COMPOSE_FILE=/home/ubuntu/ember-trove/deploy/docker-compose.prod.yml
if docker compose -f "$COMPOSE_FILE" ps proxy | grep -q 'running'; then
    docker compose -f "$COMPOSE_FILE" exec -T proxy nginx -s reload
    echo "[certbot-deploy] nginx reloaded successfully"
else
    echo "[certbot-deploy] proxy container not running — skipping reload"
fi
HOOK
sudo chmod +x /etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh
```

### Create a systemd timer

This replaces the older cron-based renewal. The timer fires twice daily, which is Certbot's recommended schedule.

```bash
sudo tee /etc/systemd/system/certbot-renew.service > /dev/null <<'SVC'
[Unit]
Description=Certbot renewal

[Service]
Type=oneshot
ExecStart=/usr/bin/certbot renew --quiet
SVC

sudo tee /etc/systemd/system/certbot-renew.timer > /dev/null <<'TIMER'
[Unit]
Description=Certbot renewal timer

[Timer]
OnCalendar=*-*-* 00,12:00:00
RandomizedDelaySec=3600
Persistent=true

[Install]
WantedBy=timers.target
TIMER

sudo systemctl daemon-reload
sudo systemctl enable --now certbot-renew.timer
```

Verify the timer is active:

```bash
systemctl status certbot-renew.timer
```

Test the renewal process (dry run):

```bash
sudo certbot renew --dry-run
```

---

## Step 13 — Customise the Cognito Hosted Login UI (optional)

The repository ships `deploy/cognito.css` and `deploy/logo.png` — a stone/amber stylesheet and flame-icon logo that match the app's visual style. Apply them with the AWS CLI:

```bash
aws cognito-idp set-ui-customization \
  --user-pool-id us-east-2_XXXXXXXXX \
  --client-id ALL \
  --image-file fileb://deploy/logo.png \
  --css "$(cat deploy/cognito.css)" \
  --region us-east-2
```

To edit the CSS, modify `deploy/cognito.css` (must stay ≤ 3 072 characters) and re-run the command. Only classes from the [Cognito allowlist](https://docs.aws.amazon.com/cognito/latest/developerguide/hosted-ui-classic-branding.html) are accepted; any unlisted class is silently ignored.

---

## Step 14 — Create Additional Users (optional)

Use the AWS CLI to add more users:

```bash
# Create the user
aws cognito-idp admin-create-user \
  --user-pool-id us-east-2_XXXXXXXXX \
  --username someone@example.com \
  --user-attributes Name=email,Value=someone@example.com Name=email_verified,Value=true \
  --message-action SUPPRESS \
  --region us-east-2

# Set a permanent password
aws cognito-idp admin-set-user-password \
  --user-pool-id us-east-2_XXXXXXXXX \
  --username someone@example.com \
  --password 'Password1!' \
  --permanent \
  --region us-east-2
```

Alternatively, users can be managed from within the app's **Admin** panel (admin role required).

---

## Automatic Deployments (CD Pipeline)

Once the repository secrets `LIGHTSAIL_HOST` and `LIGHTSAIL_SSH_KEY` are set and the repository variable `DEPLOY_ENABLED=true` is configured, every push of a `v*.*.*` tag triggers the `release.yml` GitHub Actions workflow automatically:

1. Creates a GitHub Release for the tag
2. SSH-connects to the Lightsail instance
3. Runs `docker compose build` (Rust + WASM compilation on the server)
4. Force-recreates the `api` and `ui` containers
5. Health-checks `GET /api/health` with retries (up to 60 s)

To trigger a deploy, simply tag and push:

```bash
git tag v1.2.3
git push origin main --tags
```

Monitor progress in the **Actions** tab on GitHub.

---

## Updating to a New Version (manual)

If you need to deploy without the CD pipeline:

```bash
ssh -i /path/to/key.pem ubuntu@<static-ip>
cd ember-trove
git pull origin main
docker compose -f deploy/docker-compose.prod.yml --env-file deploy/.env.prod build api ui
docker compose -f deploy/docker-compose.prod.yml --env-file deploy/.env.prod up -d --force-recreate api ui
```

To force a full rebuild without cache (e.g. after a dependency change):

```bash
docker compose -f deploy/docker-compose.prod.yml --env-file deploy/.env.prod build --no-cache api ui
```

---

## Useful Maintenance Commands

```bash
# View logs
docker logs deploy-api-1 --tail 50 -f
docker logs deploy-proxy-1 --tail 50 -f

# Restart a single service
docker compose -f deploy/docker-compose.prod.yml --env-file deploy/.env.prod restart api

# Connect to the database
docker exec -it deploy-postgres-1 psql -U ember_trove -d ember_trove

# Reset a Cognito user password
aws cognito-idp admin-set-user-password \
  --user-pool-id us-east-2_XXXXXXXXX \
  --username user@example.com \
  --password 'NewPassword1!' \
  --permanent \
  --region us-east-2
```

---

## Configuration Reference

All API configuration is set via environment variables in `deploy/.env.prod` and `deploy/docker-compose.prod.yml`.

| Variable | Required | Description |
|----------|----------|-------------|
| `DATABASE_URL` | Always | PostgreSQL connection string (set in compose, not env file) |
| `POSTGRES_PASSWORD` | Always | PostgreSQL password |
| `COOKIE_KEY` | Always | 128 hex chars (64 bytes) for cookie encryption |
| `COOKIE_SECURE` | Always | Set `true` in production |
| `FRONTEND_URL` | Always | Browser-facing URL of the UI |
| `API_EXTERNAL_URL` | Always | Browser-facing URL of the API |
| `OIDC_ISSUER` | Auth | Cognito issuer: `https://cognito-idp.<region>.amazonaws.com/<pool-id>` |
| `OIDC_CLIENT_ID` | Auth | Cognito App Client ID |
| `OIDC_CLIENT_SECRET` | Auth | Cognito App Client Secret |
| `COGNITO_USER_POOL_ID` | Admin | User Pool ID for user management API |
| `COGNITO_REGION` | Admin | AWS region of the User Pool |
| `AWS_ACCESS_KEY_ID` | Admin | IAM access key for Cognito admin operations |
| `AWS_SECRET_ACCESS_KEY` | Admin | IAM secret key for Cognito admin operations |
| `S3_BUCKET` | S3 | Object Storage bucket name |
| `S3_ACCESS_KEY` | S3 | Bucket access key |
| `S3_SECRET_KEY` | S3 | Bucket secret key |
| `S3_REGION` | S3 | Bucket region |
| `RUST_LOG` | Optional | Log level: `error`, `warn`, `info`, `debug` (default: `info`) |

---

## Troubleshooting

### 502 Bad Gateway on `/api/auth/callback`

nginx's default proxy buffer (4 KB) is too small for JWT `Set-Cookie` headers. The fix is already applied in `nginx.prod.conf` (`proxy_buffer_size 128k`). If you see this error, ensure you are using the production nginx config and not the dev one.

### Logout immediately re-authenticates

The logout handler redirects through Cognito's `end_session_endpoint` to clear the Cognito SSO session cookie. If logout loops back to the app, verify that `FRONTEND_URL` in `docker-compose.prod.yml` exactly matches the `logout-urls` configured for the App Client in Cognito (Step 3b).

### Username shows as UUID

The sidebar falls back to `email` if the `name` claim is absent (Cognito does not populate `name` by default). If you still see a UUID, check that `email` scope is listed in `--allowed-o-auth-scopes` for the App Client.

### Docker build is fully cached after a `git pull`

This happens when the server is on a branch that doesn't include the latest commits. Verify with `git log --oneline -3` and ensure you are on `main` (or the release tag you intend to deploy). Then rebuild with `--no-cache`.

### Certificate not renewing

Run `sudo certbot renew --dry-run` and inspect the output. Common causes: port 80 blocked in the Lightsail firewall, or the deploy hook path is wrong.
