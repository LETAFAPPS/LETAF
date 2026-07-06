//! Página HTML hosted de tokenização de cartão (Efi.js).
//!
//! Regras aplicadas (AI_RULES.md §3, §11):
//! - HTML/JS aqui é uma EXCEÇÃO justificada ao §3: a Efi descontinuou a
//!   tokenização server-side; a única forma suportada é a lib Efi.js no
//!   navegador. É uma ponte de tokenização, não a UI do app.
//! - O cartão (PAN/CVV) é tokenizado no navegador e NUNCA é enviado ao
//!   nosso server — só o `payment_token` resultante.

/// Renderiza a página de cadastro com o `payee_code` da conta, o plano
/// e o valor. `cobrancas_base` é a base da API Cobranças (define o CDN
/// do Efi.js: homologação x produção).
pub fn render(
    cobrancas_base: &str,
    payee_code: &str,
    session_token: &str,
    plan_label: &str,
    amount: f64,
) -> String {
    let amount_display = format!("R$ {:.2}", amount).replace('.', ",");
    TEMPLATE
        .replace("{{CDN}}", cobrancas_base)
        .replace("{{PAYEE}}", payee_code)
        .replace("{{TOKEN}}", session_token)
        .replace("{{PLAN}}", &html_escape(plan_label))
        .replace("{{AMOUNT}}", &amount_display)
}

pub fn error_page(message: &str) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"pt-BR\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>LETAF</title></head><body style=\"font-family:system-ui;background:#0f1320;color:#e7e9ee;\
         display:flex;align-items:center;justify-content:center;height:100vh;margin:0\">\
         <div style=\"text-align:center;padding:24px\"><h2>Não foi possível abrir o cadastro</h2>\
         <p style=\"color:#9aa0ad\">{}</p></div></body></html>",
        html_escape(message)
    )
}

/// Escapa o mínimo para inserir texto em HTML com segurança.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const TEMPLATE: &str = r##"<!DOCTYPE html>
<html lang="pt-BR">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Cadastrar cartão · LETAF</title>
<style>
  :root{--bg:#0f1320;--card:#171c2b;--bd:#2a3145;--tx:#e7e9ee;--mut:#9aa0ad;--pri:#f97316;--err:#ef4444;--ok:#22c55e}
  *{box-sizing:border-box}
  body{font-family:system-ui,Segoe UI,Roboto,sans-serif;background:var(--bg);color:var(--tx);margin:0;padding:24px}
  .wrap{max-width:520px;margin:0 auto}
  h1{font-size:20px;margin:0 0 4px}
  .sub{color:var(--mut);font-size:13px;margin:0 0 18px}
  .card{background:var(--card);border:1px solid var(--bd);border-radius:14px;padding:20px}
  label{display:block;font-size:12px;color:var(--mut);margin:10px 0 4px}
  input,select{width:100%;background:#0e1220;border:1px solid var(--bd);border-radius:8px;color:var(--tx);
    font-size:14px;padding:10px}
  input:focus,select:focus{outline:none;border-color:var(--pri)}
  .row{display:flex;gap:12px}.row>div{flex:1}
  .sec{font-size:12px;font-weight:700;color:var(--mut);margin:18px 0 2px;border-top:1px solid var(--bd);padding-top:14px}
  button{margin-top:18px;width:100%;background:var(--pri);border:0;border-radius:8px;color:#111;
    font-size:15px;font-weight:700;padding:13px;cursor:pointer}
  button:disabled{opacity:.6;cursor:default}
  #msg{margin-top:14px;font-size:13px;padding:10px;border-radius:8px;display:none}
  #msg.err{display:block;background:#2a1417;color:var(--err)}
  #msg.ok{display:block;background:#10261a;color:var(--ok)}
  .pay{font-size:13px;color:var(--mut)}
</style>
</head>
<body>
<div class="wrap">
  <h1>Cadastrar cartão de crédito</h1>
  <p class="sub">LETAF · Plano {{PLAN}} · <b>{{AMOUNT}}</b> · cobrança automática recorrente</p>
  <div class="card">
    <form id="f" autocomplete="on">
      <div class="sec">Cartão</div>
      <label>Número do cartão</label>
      <input id="number" inputmode="numeric" placeholder="0000 0000 0000 0000" maxlength="23" required>
      <label>Nome impresso no cartão</label>
      <input id="holder" placeholder="Como está no cartão" required>
      <div class="row">
        <div><label>Validade (MM/AA)</label><input id="expiry" inputmode="numeric" placeholder="08/30" maxlength="5" required></div>
        <div><label>CVV</label><input id="cvv" inputmode="numeric" placeholder="123" maxlength="4" required></div>
      </div>

      <div class="sec">Titular (antifraude)</div>
      <div class="row">
        <div><label>CPF/CNPJ</label><input id="cpf" inputmode="numeric" placeholder="000.000.000-00" required></div>
        <div><label>Nascimento</label><input id="birth" type="date" required></div>
      </div>
      <div class="row">
        <div><label>E-mail</label><input id="email" type="email" placeholder="voce@empresa.com" required></div>
        <div><label>Telefone</label><input id="phone" inputmode="numeric" placeholder="(11) 99999-8888" required></div>
      </div>

      <div class="sec">Endereço de cobrança</div>
      <div class="row">
        <div><label>CEP</label><input id="cep" inputmode="numeric" placeholder="00000-000" required></div>
        <div><label>Número</label><input id="snumber" placeholder="123" required></div>
      </div>
      <label>Logradouro</label>
      <input id="street" placeholder="Rua / Avenida" required>
      <label>Bairro</label>
      <input id="neighborhood" placeholder="Bairro" required>
      <div class="row">
        <div><label>Cidade</label><input id="city" placeholder="Cidade" required></div>
        <div><label>UF</label><input id="state" placeholder="SP" maxlength="2" required></div>
      </div>

      <button id="btn" type="submit">Cadastrar cartão</button>
      <p class="pay" style="margin-top:10px">Os dados do cartão são protegidos e tokenizados pela Efí no seu navegador — não passam pelos nossos servidores.</p>
    </form>
    <div id="msg"></div>
  </div>
</div>

<script type="text/javascript">
  // Carrega a lib de tokenização da Efi (Efi.js) para esta conta.
  var s = function () {
    var rand = Math.floor(Math.random() * 1000000);
    window.$gn = { validForm: true, processed: false, done: {}, ready: function (fn) { window.$gn.done = fn; } };
    var i = document.createElement('script');
    i.type = 'text/javascript'; i.async = true;
    i.src = '{{CDN}}/v1/cdn/{{PAYEE}}/' + rand;
    i.id = String(rand);
    i.onerror = function () { msg('Não foi possível carregar o componente seguro da Efí. Verifique sua conexão.', 'err'); };
    document.getElementsByTagName('head')[0].appendChild(i);
  };
  s();

  var checkout = null;
  window.$gn.ready(function (c) { checkout = c; });

  function val(id){ var e=document.getElementById(id); return e?e.value.trim():''; }
  function dig(id){ return val(id).replace(/\D/g,''); }
  function msg(t,c){ var m=document.getElementById('msg'); m.textContent=t; m.className=c; }

  function brandOf(n){
    if(/^4/.test(n)) return 'visa';
    if(/^(5[1-5]|2[2-7])/.test(n)) return 'mastercard';
    if(/^3[47]/.test(n)) return 'amex';
    if(/^(606282|3841)/.test(n)) return 'hipercard';
    if(/^(4011|4312|4514|4576|5041|5066|5067|509|6277|6362|6363|650|651|655)/.test(n)) return 'elo';
    return 'visa';
  }

  // máscara simples de validade
  document.getElementById('expiry').addEventListener('input', function(e){
    var d=e.target.value.replace(/\D/g,'').slice(0,4);
    e.target.value = d.length>2 ? d.slice(0,2)+'/'+d.slice(2) : d;
  });

  document.getElementById('f').addEventListener('submit', function (e) {
    e.preventDefault();
    if (!checkout) { msg('Aguarde o componente seguro carregar e tente de novo.', 'err'); return; }
    var num = dig('number');
    var exp = dig('expiry');
    if (exp.length !== 4) { msg('Validade incompleta (MM/AA).', 'err'); return; }
    var brand = brandOf(num);
    document.getElementById('btn').disabled = true;
    msg('Tokenizando com a Efí...', 'ok');
    checkout.getPaymentToken({
      brand: brand,
      number: num,
      cvv: val('cvv'),
      expiration_month: exp.slice(0,2),
      expiration_year: '20' + exp.slice(2)
    }, function (error, response) {
      if (error) {
        document.getElementById('btn').disabled = false;
        msg('Cartão inválido: ' + (error.error_description || error.error || 'erro na tokenização'), 'err');
        return;
      }
      var pt = response.data.payment_token;
      submitToken(pt, brand, num.slice(-4));
    });
  });

  function submitToken(payment_token, brand, last4) {
    msg('Processando o cadastro...', 'ok');
    var body = {
      session_token: '{{TOKEN}}',
      payment_token: payment_token,
      brand: brand,
      last4: last4,
      expiry: val('expiry'),
      name: val('holder'),
      cpf: dig('cpf'),
      email: val('email'),
      phone: dig('phone'),
      birth: val('birth'),
      cep: dig('cep'),
      street: val('street'),
      number: val('snumber'),
      neighborhood: val('neighborhood'),
      city: val('city'),
      state: val('state').toUpperCase()
    };
    fetch('/pay/card/submit', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body)
    }).then(function (r) { return r.json(); }).then(function (d) {
      if (d.ok) {
        document.getElementById('f').style.display = 'none';
        msg('Cartão cadastrado com sucesso! Pode fechar esta aba e voltar ao LETAF.', 'ok');
      } else {
        document.getElementById('btn').disabled = false;
        msg('Erro: ' + (d.error || 'falha ao cadastrar'), 'err');
      }
    }).catch(function (e) {
      document.getElementById('btn').disabled = false;
      msg('Falha de rede: ' + e, 'err');
    });
  }
</script>
</body>
</html>
"##;
