export JWT="$(
    curl -sS \
      -u "$ID:$SECRET" \
      -d 'grant_type=client_credentials' \
      -d 'scope=zela-executor:call' \
      'https://auth.zela.io/realms/zela/protocol/openid-connect/token' \
    | jq -r '.access_token'
  )"

echo $JWT
