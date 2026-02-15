export JWT="$(
    curl -sS \
      -u "$CLIENT_ID:$CLIENT_SECRET" \
      -d 'grant_type=client_credentials' \
      -d 'scope=zela-executor:call' \
      'https://auth.zela.io/realms/zela/protocol/openid-connect/token' \
    | jq -r '.access_token'
  )"
