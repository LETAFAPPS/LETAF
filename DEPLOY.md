# LETAF — Deploy & Segurança de produção

Checklist e configurações para colocar o servidor (`letaf-server`) em
produção com segurança. Itens marcados ⚠️ vieram da auditoria e devem
ser resolvidos antes de expor o servidor publicamente.

---

## 1. ⚠️ Rate limiting (anti brute-force) — na BORDA

O rate limiting é feito no **proxy reverso / CDN**, não na aplicação.
Motivo: por trás de um proxy, todos os clientes chegam com o IP do
proxy; um limite por-IP feito no app colapsaria todos no mesmo IP e
poderia **bloquear todos os logins legítimos**. A borda enxerga o IP
real e é o lugar correto.

Proteja principalmente os endpoints de autenticação:
`/auth/login`, `/auth/login-desktop`, `/customer/login`, `/customer/register`.

### nginx

```nginx
# 10 req/min por IP, com pequeno burst (ajuste conforme seu tráfego).
limit_req_zone $binary_remote_addr zone=login:10m rate=10r/m;

server {
    # ... TLS, proxy_pass para o letaf-server (porta 3001) ...

    location = /auth/login          { limit_req zone=login burst=5 nodelay; proxy_pass http://127.0.0.1:3001; }
    location = /auth/login-desktop  { limit_req zone=login burst=5 nodelay; proxy_pass http://127.0.0.1:3001; }
    location = /customer/login      { limit_req zone=login burst=5 nodelay; proxy_pass http://127.0.0.1:3001; }
    location = /customer/register   { limit_req zone=login burst=3 nodelay; proxy_pass http://127.0.0.1:3001; }

    location / { proxy_pass http://127.0.0.1:3001; }
}
```

### Cloudflare
Painel → **Security → WAF → Rate limiting rules**: criar regra para
`URI Path contém "/login"` ou `/auth/` com limite (ex.: 10 req/min por
IP), ação *Block* por alguns minutos.

> Se um dia for necessário rate limit no app (sem proxy, ou com proxy
> confiável repassando `X-Forwarded-For`/`CF-Connecting-IP`), reabrir o
> assunto informando a topologia — aí dá para fazer por-IP corretamente.

---

## 2. ⚠️ Webhook PIX (Pix Automático) — autenticar a origem

`POST /webhooks/efi/pix` aplica confirmação de pagamento. Para impedir
forja, faça **uma** das opções:

- **HMAC (já implementado):** configure `EFI_PIX_WEBHOOK_HMAC=<segredo>`
  no `.env` **e** o mesmo segredo no painel da Efi (HMAC do webhook). A
  Efi passa a anexar `?hmac=<segredo>` à URL; o servidor rejeita (401)
  quem não tiver o hmac correto.
- **mTLS:** validar o certificado de cliente da Efi no proxy reverso
  (terminação TLS) antes de encaminhar ao app.

> Validar em **homologação Efi** antes de produção. Sem nenhuma das duas,
> o endpoint aceita o corpo de qualquer origem.

---

## 3. CORS

Defina `CORS_ORIGINS` com os domínios reais (nunca `*` em produção):
```
CORS_ORIGINS=https://app.seusite.com,https://*.seusite.com
```

## 4. Segredos
`.env`, `*.p12` (certificados Efi) e `letaf.db` **não** são versionados
(ver `.gitignore`). Em produção:
- `JWT_SECRET`: obrigatório (o release dá panic se ausente) — use um
  valor forte e único.
- Certificados `.p12` e `EFI_*`: providos via ambiente seguro.

**Onde ficam os certificados (desenvolvimento):** FORA da árvore do
repositório, em `~/.letaf/secrets/` (modo `700`, arquivos `600`), e o
`.env` aponta com caminho ABSOLUTO:

```
EFI_P12_PATH="/home/<usuário>/.letaf/secrets/<arquivo>.p12"
```

Guardá-los na raiz do projeto era arriscado mesmo estando no
`.gitignore`: um `git add -f`, um zip do diretório ou um backup da pasta
levava a chave junto. O caminho absoluto também remove a dependência de
rodar o servidor a partir da raiz do repo (antes o caminho era relativo).

Para conferir se o certificado foi carregado, basta o log do boot: a
linha `Efi (PIX) habilitada · env=…` só é emitida quando o `.p12` é lido
com sucesso e o mTLS é montado (`EfiClient::new`); se o caminho ou a
senha estiverem errados, o servidor sobe sem PIX e loga o erro.

## 5. Build de produção
```bash
cargo run -p letaf-server --bin letaf-server --release   # ou cargo build --release
```
O perfil `release` já aplica LTO, `strip`, `panic=abort` e `opt-level=3`.

---

## 6. Publicar atualização do desktop

O desktop checa `GET /app/version` na inicialização e a cada 6h, compara a
versão embutida (`CARGO_PKG_VERSION`) com o manifesto via semver e exibe um
modal quando há versão nova. Tudo é servido pelo próprio backend a partir de
`APP_UPDATES_DIR` (default `updates/`).

O "Atualizar agora" faz **auto-update**: baixa o binário, valida o `sha256`
do manifesto, substitui o executável (self-replace) e reinicia. Por isso os
binários publicados são **brutos** (o próprio executável), não instaladores.

Para publicar a versão `X.Y.Z`:
1. **Bump** da versão em `desktop/Cargo.toml` (`version = "X.Y.Z"`), commit e
   crie a tag `vX.Y.Z` (`git tag vX.Y.Z && git push --tags`).
2. O **CI** (`.github/workflows/release.yml`) builda os 3 SOs, calcula o
   `sha256` e publica no **GitHub Release** os binários (`letaf-linux`,
   `letaf-windows.exe`, `letaf-macos`) + um `manifest.json` já preenchido.
3. **Baixe** esses arquivos do Release e copie para o `APP_UPDATES_DIR` do
   servidor. Ajuste `min_supported`/`notes` no `manifest.json` conforme a
   política (deixe `min_supported` = versão abaixo da qual a atualização
   passa a ser OBRIGATÓRIA; `null` = sempre opcional):
   ```json
   {
     "latest": "X.Y.Z",
     "min_supported": "A.B.C",
     "notes": "O que mudou…",
     "files":  { "linux": "letaf-linux", "windows": "letaf-windows.exe", "macos": "letaf-macos" },
     "sha256": { "linux": "<hex>", "windows": "<hex>", "macos": "<hex>" }
   }
   ```
   O desktop monta a URL como `{server_url}/app/download/{arquivo}`.

Sem CI, dá para gerar os binários localmente (`cargo build -p letaf-desktop
--release` em cada SO) e montar o manifesto à mão (`sha256sum` em cada um).

Não precisa reiniciar o servidor — o manifesto é lido a cada requisição.
Sem `manifest.json`, o endpoint responde 204 (nenhuma atualização). A rota
de download bloqueia path traversal (só nome de arquivo simples).

> **macOS:** o auto-update via self-replace troca o binário bruto; apps
> empacotados em `.app` assinados/notarizados exigem fluxo próprio — por ora
> o fallback "Baixar manual" cobre esse caso.

---

## Pendências da auditoria (não-bloqueantes, recomendadas)
- [ ] Rate limiting na borda (seção 1).
- [ ] Webhook PIX autenticado (seção 2) + teste em homologação Efi.
- [ ] Desempenho do sync do desktop (`pull_all`): medir e, se necessário,
      avançar cursor por entidade em vez de global.
- [ ] Refactor de arquivos grandes (`desktop/src/sync/worker.rs` etc.) —
      apenas organização, sem mudança de comportamento.
