#!/usr/bin/env sh
# Creates .env for the compose file: asks for the domain and the e-mail, generates the secret key. Never overwrites an existing .env.
set -eu
cd "$(dirname "$0")"
if [ -e .env ]; then echo ".env exists, leaving it alone"; exit 0; fi
printf 'Domain Hermes is reached at [localhost]: '; read -r domain; domain=${domain:-localhost}
printf "E-mail for Let's Encrypt [admin@%s]: " "$domain"; read -r mail; mail=${mail:-admin@$domain}
umask 077
sed -e "s|^DOMAIN=.*|DOMAIN=$domain|" -e "s|^ACME_EMAIL=.*|ACME_EMAIL=$mail|" \
    -e "s|^HUB_SECRET_KEY=.*|HUB_SECRET_KEY=$(openssl rand -hex 32)|" .env.example > .env
echo ".env written. Back it up: without HUB_SECRET_KEY the stored credentials cannot be read. Then: docker compose up -d"
