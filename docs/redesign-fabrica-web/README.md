# Redesign do Web — tema Fábrica (site institucional)

Proposta visual (planejamento + mockups) para o tema **fábrica** do web (crate `web`, Leptos
SSR). Diferente de restaurante e loja, a fábrica é um **site institucional SEM e-commerce**:
apresenta a empresa e a produção e capta **orçamentos/contatos** — sem carrinho, preços ou
checkout. Mantém a identidade teal do LETAF (`--brand: #0e7490`), SSR/SEO por subdomínio e
frontend burro (AI_RULES §3/§11).

> Status: **somente planejamento**. Nada foi aplicado ao código. Aguardando aprovação.

## Arquivos

- [`plano.html`](plano.html) — documento visual completo e **self-contained** (abre no
  navegador; imagens embutidas). Design system, mockups, plano por seção, o que sai/entra
  vs. temas de venda, e roadmap. Também publicado como Artifact.
- [`mockups/`](mockups/) — as 6 telas em alta resolução (geradas por IA, ilustrativas):
  - `fab_home_light.png` — home institucional, tema claro
  - `fab_home_dark.png` — home institucional, tema escuro
  - `fab_products.png` — mostruário de produtos (sem preços)
  - `fab_detail.png` — detalhe de produto (ficha técnica + orçamento)
  - `fab_contact.png` — contato / solicitar orçamento
  - `fab_mobile.png` — home no celular

## O que muda vs. restaurante/loja

- **Sai**: carrinho, checkout, preços, botão "adicionar", login de cliente.
- **Vira mostruário**: produtos como referência (foto + ficha técnica), sem compra.
- **Entra**: hero institucional, números da empresa, "Sobre", diferenciais, formulário de
  **orçamento/lead** e rodapé institucional.
- **Continua**: SSR/SEO por tenant (ainda mais importante aqui), design system em tokens
  (`data-theme="fabrica"`), multi-tenant.

## Dependência de backend

O **formulário de orçamento/contato** precisa de um endpoint de lead no `server`
(validação e autoridade no backend). O restante do conteúdo institucional é renderável a
partir dos dados que o web já carrega (empresa, produtos, contato).

## Roadmap (após aprovação)

1. Fundação — tokens & layout institucional (navegação por âncoras) + modo escuro
2. Home institucional (header, hero, números, sobre, diferenciais)
3. Mostruário de produtos + modal de ficha técnica (sem preço/carrinho)
4. Contato/Orçamento + endpoint de lead no server
5. Acabamento — rodapé, microinterações, acessibilidade, QA responsivo, SEO/SSR

## Observação sobre os mockups

Imagens geradas por IA para dar a sensação do visual — layout, paleta e componentes são a
proposta concreta, mas textos/detalhes podem ter pequenas imperfeições. Na implementação
real (Leptos + SCSS) tudo vem de tokens e fica preciso.

Temas irmãos: [`../redesign-cardapio-web/`](../redesign-cardapio-web/) (restaurante) ·
[`../redesign-loja-web/`](../redesign-loja-web/) (loja).
