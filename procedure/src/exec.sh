curl -sS \
    -H "Authorization: Bearer $JWT" \
    -H "Content-Type: application/json" \
    --data "{
      \"jsonrpc\":\"2.0\",
      \"id\":1,
      \"method\":\"zela.${PROC}#${HASH}\",
      \"params\":[{}]
    }" \
    https://executor.zela.io
