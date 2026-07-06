# LETAF — instruções do projeto

ERP multi-tenant, **100% Rust** (workspace: `core` domínio puro · `server`
API axum/PostgreSQL · `desktop` Slint/SQLite · `web` Leptos SSR/SEO).
Offline-first com sync híbrido.

## Regras obrigatórias

As regras completas e **vinculantes** estão em **@AI_RULES.md** — leia e
siga TODAS. Em qualquer conflito, o `AI_RULES.md` prevalece.

## Invariantes (resumo — detalhe, exemplos e exceções no @AI_RULES.md)

- Falar com o usuário em **português brasileiro**; código e identificadores
  em inglês; comentários em pt-BR.
- **Antes de gerar código (§15):** (1) explicar a arquitetura da solução,
  (2) garantir que nenhuma regra é violada, (3) só então gerar. Se não for
  possível seguir uma regra → NÃO gerar; explicar o motivo.
- **Multi-tenant:** toda query filtra por `company_id`; nunca vazar dados
  entre empresas.
- **Offline-first:** escreve no SQLite → `synced=false` → tenta sync →
  fila; nunca bloquear a UI aguardando rede.
- **Modelagem:** id UUID (sem auto-incremento), soft delete (`deleted_at`),
  acesso a banco somente via `repository`.
- **Frontend burro / não-confiável:** desktop e web só renderizam, coletam
  input e chamam a API; toda validação, autoridade e decisão de segurança
  vivem no backend. O backend nunca confia em valores do frontend.
- **Rust:** 100% safe — **proibido `unsafe`**. `cargo clippy` sem warnings
  é critério de "pronto". Idiomático, zero-cost, concorrência correta.
- **Clean code:** responsabilidade única de arquivos e funções; sem lógica
  de negócio na UI; sem duplicação.
