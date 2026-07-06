# 🧠 REGRAS OBRIGATÓRIAS DO PROJETO ERP (100% RUST)

# Idioma
- Toda comunicação com o usuário deve ser em português brasileiro (pt-BR).
- Explicações, resumos e justificativas de mudanças: em português.
- Código, nomes de variáveis e identificadores: manter em inglês (padrão).
- Comentários no código: em português, salvo se o projeto já estiver em inglês.

---

## Multi-tenant e subdomínios

O sistema deve suportar múltiplas empresas (multi-tenant).

Cada empresa terá um subdomínio único:

- empresa1.seusite.com
- empresa2.seusite.com

### Regras obrigatórias:

- O backend deve identificar a empresa pelo subdomínio da requisição (Host)
- O sistema deve mapear subdomínio → company_id
- Todas as operações devem ser isoladas por company_id
- Nenhuma operação pode acessar dados de outra empresa
- O frontend web deve carregar dados dinamicamente com base no subdomínio
- O domínio principal (seusite.com) será apenas institucional

---

## Identificação da empresa no backend

- O backend deve extrair o subdomínio a partir do header "Host"
- Deve existir um middleware responsável por:
  - identificar o subdomínio
  - resolver o company_id
  - injetar o company_id no contexto da requisição
- Nenhum endpoint deve funcionar sem company_id definido

---

## Isolamento de dados

- Todas as entidades devem conter company_id
- Todas as queries devem obrigatoriamente filtrar por company_id
- É PROIBIDO qualquer acesso sem isolamento por empresa

---

## Desktop (ERP local)

- O desktop representa apenas UMA empresa
- O company_id deve estar fixo localmente
- O sistema deve funcionar offline
- Sincronização com servidor deve respeitar o company_id

---

## Web (clientes finais)

- O site acessado via subdomínio pertence a uma única empresa
- O cliente final não escolhe empresa manualmente
- A empresa é identificada automaticamente pelo subdomínio

---

## Princípios fundamentais de engenharia

Princípios transversais que valem para TODO o código do projeto. Quando
conflitarem com conveniência, prevalecem os princípios.

### Rust idiomático
- Escrever Rust idiomático: `Result`/`Option` em vez de sentinelas, `?`
  para propagação de erro, iteradores no lugar de loops manuais quando
  for mais claro, pattern matching exaustivo, `From`/`TryFrom`/`Default`.
- O código deve compilar SEM warnings: `cargo clippy` limpo é condição
  para considerar uma mudança pronta.

### Zero-cost abstractions
- Preferir abstrações resolvidas em tempo de compilação (genéricos
  monomorfizados, iteradores, `impl Trait`) a indireções de runtime
  quando não houver ganho real.
- `dyn Trait` / `Box` / `Arc` apenas quando inversão de dependência ou
  compartilhamento justificam (ex.: repositories via trait — §10).
- Não pagar por alocação, cópia ou indireção evitável.

### Memória segura — proibido `unsafe`
- É PROIBIDO escrever `unsafe` no nosso código. A base deve ser 100%
  safe Rust.
- Se surgir uma necessidade aparente de `unsafe`, repensar o design ou
  usar uma crate revisada que já encapsule o `unsafe` com segurança.

### Concorrência otimizada e correta
- Concorrência via Tokio (async/await); nunca bloquear o executor com
  trabalho síncrono pesado (usar `spawn_blocking` quando necessário).
- Estado compartilhado protegido corretamente (`Arc` + `RwLock`/`Mutex`),
  segurando locks pelo menor tempo possível; nunca manter um lock
  síncrono atravessando um `.await`.
- Sem data races e sem deadlocks; preferir canais/mensagens a estado
  mutável compartilhado quando isso simplificar o raciocínio.

### Modularidade real (backend e frontend)
- Modularidade vale para AS DUAS pontas — backend (core/server) e
  frontend (desktop/web). Detalha e reforça §1 e §9.
- Cada módulo expõe uma fronteira clara; dependências fluem em uma só
  direção (UI → service → repository; nunca o contrário).

### Responsabilidade única (arquivos e funções)
- Reforça §8: cada arquivo e cada função fazem UMA coisa.
- Arquivo que cresce demais é quebrado em submódulos por
  responsabilidade; função longa é extraída.

### Tempo real, responsivo e adaptativo
- Operações refletem na UI em tempo real, sem travar a interface (§7.4).
- Layout responsivo e adaptativo a diferentes tamanhos de tela/janela,
  tanto no desktop (Slint) quanto na web (Leptos) — §3.

### Prioridade: segurança e desempenho
- Segurança (§11) e desempenho (§13) são prioridades de primeira classe,
  não pensamentos posteriores.
- Desempenho guiado por medição (sem otimização prematura), mas sem
  introduzir custos óbvios e evitáveis.

### UX e UI moderna e bonita
- Interface moderna, limpa e agradável: hierarquia visual clara,
  espaçamento consistente, feedback imediato (toasts, estados de
  carregamento) e acessibilidade básica (contraste, alvos de toque).
- A estética é requisito, não opcional — sem nunca colocar lógica de
  negócio na UI (§3).

---

## 1. Arquitetura

O sistema deve seguir arquitetura modular em camadas:

- core → regras de negócio (domínio puro)
- server → API backend
- desktop → interface desktop
- web → interface web

Regras:
- É PROIBIDO misturar responsabilidades entre camadas
- UI nunca pode conter lógica de negócio
- Core não pode depender de UI ou banco diretamente

---

## 2. Linguagem

- Todo o código deve ser escrito em Rust
- Não sugerir outras linguagens (exceto serviços externos como IA, se explicitamente solicitado)

---

## 3. Frontend

- Desktop deve usar Slint
- Web deve usar Leptos (Rust), com SSR (server-side rendering) + hidratação
- SEO é requisito: cada cardápio (subdomínio) é renderizado no servidor —
  HTML com conteúdo real + meta tags por tenant (título/descrição/og),
  indexável por buscadores. O tenant é resolvido pelo header `Host`, igual
  ao middleware multi-tenant.
- UI deve ser responsiva e adaptável

Proibições:
- Não usar HTML/JS fora do Leptos
- Não colocar lógica de negócio no frontend — o frontend é um cliente
  "burro" e não-confiável; toda regra/validação/segurança vive no backend
  (vide §11 "Frontend burro"). O SSR renderiza apenas conteúdo PÚBLICO
  (catálogo); login/carrinho/checkout seguem thin-client → API com JWT.
- A camada SSR fica separada/explícita (crate próprio ou rota dedicada no
  axum), sem misturar com a API REST (§1).

---

## 4. Backend

- Usar axum para API
- Usar Tokio para execução assíncrona
- Usar SQLx para acesso ao banco

Padrão:
- API REST
- JSON como formato de resposta

### Transações

- Operações críticas devem ser executadas em transações
- Exemplos:
  - criação de venda
  - baixa de estoque
  - movimentação financeira

---

## 5. Banco de dados

- Desktop → SQLite
- Servidor → PostgreSQL

Regras:
- Nunca misturar os bancos diretamente
- Nunca acessar banco fora da camada repository

---

## 6. Modelagem de dados

Toda entidade deve conter:

- id (UUID)
- company_id
- created_at
- updated_at
- deleted_at (opcional)
- synced (boolean)

Proibições:
- Nunca usar auto-incremento

### Remoção de dados

- A remoção deve ser lógica (soft delete)
- Utilizar o campo deleted_at
- Dados não devem ser removidos fisicamente do banco

---

## 7. Sincronização (offline-first com sync híbrido)

O sistema deve seguir obrigatoriamente o modelo híbrido.

### 7.1 Princípios

- O sistema deve ser offline-first
- Nunca depender de conexão ativa
- Toda escrita deve ocorrer primeiro no SQLite

---

### 7.2 Campo obrigatório

Toda entidade deve conter:

- synced (boolean)

Significado:
- false → não sincronizado
- true → sincronizado

---

### 7.3 Fluxo de escrita (OBRIGATÓRIO)

1. Salvar no SQLite
2. Marcar synced = false
3. Tentar sincronização imediata
4. Se sucesso → synced = true
5. Se falha → manter synced = false e adicionar à fila

---

### 7.4 Tempo real

- Deve tentar sincronizar imediatamente após cada operação
- Não pode bloquear o usuário

---

### 7.5 Fallback (fila)

- Deve existir um worker em background que:
  - busca dados com synced = false
  - tenta reenviar periodicamente

---

### 7.6 Resiliência

- Nenhuma falha de rede pode impedir uso do sistema
- Nenhum dado pode ser perdido
- Deve haver consistência eventual entre SQLite e PostgreSQL

---

### 7.7 Conflitos de sincronização

- Em caso de conflito, usar updated_at como referência
- O dado mais recente deve prevalecer (last-write-wins)
- O sistema deve permitir evolução futura para estratégias mais avançadas

---

### 7.8 Proibições

- Não salvar direto no servidor
- Não depender de internet para operações críticas
- Não bloquear UI aguardando API

---

## 8. Código (Clean Code)

Regras obrigatórias:

- Código deve ser modular
- Cada módulo deve ter responsabilidade única
- Cada função deve ter no máximo 30–50 linhas
- Cada função deve fazer apenas UMA coisa
- Arquivos não devem ser muito grande (Não conter linhas de mais)
- Nomes devem ser claros e descritivos
- Evitar duplicação de código
- Código deve priorizar legibilidade e manutenibilidade

---

## 9. Estrutura de módulos

Cada domínio deve seguir:

core/<modulo>/
- model.rs
- service.rs
- repository.rs

---

## 10. Acesso a dados

- Acesso ao banco somente via repository
- Usar traits para abstração

---

## 11. Segurança

- Nunca expor dados entre empresas
- Validar todos os dados de entrada no backend
- Nunca confiar em dados vindos do frontend
- Preparar autenticação (JWT ou similar)

### Frontend burro (cliente não-confiável)

O frontend (desktop e web) é tratado como NÃO-CONFIÁVEL e deve ser
"burro" de propósito: ele renderiza, coleta input e chama a API — NUNCA
é a fonte de verdade de regra de negócio nem de autorização.

- Toda validação, todo cálculo sensível, toda checagem de permissão e
  toda decisão de segurança ocorrem no backend (server). Repetir no
  frontend é apenas UX/feedback imediato — nunca a garantia.
- O backend NUNCA confia em valores vindos do frontend (preços, totais,
  `company_id`, `role`, flags): sempre recalcula/reconfere a partir da
  fonte de verdade. Exemplos no código: `order::service::verify_item_prices`
  recomputa o preço unitário; o `company_id` vem do tenant/JWT, jamais do
  corpo da requisição.
- Comprometer o cliente (DevTools, binário adulterado, requisição
  forjada) não pode quebrar a segurança nem a integridade dos dados.
- É o mesmo "sem lógica de negócio na UI" de §1/§3, aqui pelo ângulo da
  segurança: o frontend ser burro É uma medida de segurança.

---

## 12. API

Padrão REST:

- GET → leitura
- POST → criação
- PUT/PATCH → atualização
- DELETE → remoção lógica

Regras:
- Respostas sempre em JSON

---

## 13. Desempenho

- Priorizar legibilidade e manutenibilidade
- Otimização deve ser baseada em medição real (profiling)
- Evitar otimizações prematuras

---

## 14. Proibições gerais

- Não misturar camadas
- Não acessar banco na UI
- Não criar funções grandes
- Não duplicar lógica
- Não ignorar company_id
- Não ignorar fluxo de sincronização

---

## 15. Antes de gerar código (OBRIGATÓRIO)

A IA deve SEMPRE:

1. Explicar a arquitetura da solução
2. Garantir que nenhuma regra será violada
3. Só então gerar o código

Se qualquer regra não puder ser seguida:

→ NÃO gerar código
→ Explicar o motivo
