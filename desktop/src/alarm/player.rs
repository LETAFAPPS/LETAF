use std::io::Cursor;
use std::sync::mpsc::{self, Sender};
use std::thread;

use rodio::{Decoder, OutputStream, Sink};

/// Som do alarme — MP3 embutido no executável via `include_bytes!`.
///
/// Embutir (em vez de carregar do disco em runtime) garante que o
/// alarme **sempre toca**, mesmo se o operador mover/deletar arquivos
/// no diretório de instalação. Também resolve o caminho relativo no
/// Windows (onde `current_dir` pode mudar dependendo de como o app é
/// iniciado — atalho, menu Iniciar, terminal).
///
/// Para trocar o som, basta substituir `desktop/assets/sounds/alarm.mp3`
/// e recompilar. O arquivo é avaliado em compile-time.
const ALARM_BYTES: &[u8] = include_bytes!("../../assets/sounds/alarm.mp3");

/// Comandos enviados para a thread de áudio.
///
/// O `OutputStream` da rodio é `!Send` (ponteiros pra device do SO),
/// então o stream vive numa thread dedicada e o resto do código
/// (callbacks tokio, event loop Slint) só conversa via canal mpsc.
enum Cmd {
    Play,
    Stop,
}

/// Player do alarme de novos pedidos.
///
/// `AlarmPlayer::new()` sempre devolve uma instância: em ambientes sem
/// device de áudio (CI, headless), o player degrada para no-op + warn,
/// preservando a UI visual do alarme.
pub struct AlarmPlayer {
    tx: Sender<Cmd>,
}

impl AlarmPlayer {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>();
        thread::Builder::new()
            .name("alarm-audio".into())
            .spawn(move || run_audio_thread(rx))
            .ok();
        Self { tx }
    }

    /// Inicia o loop do alarme. Idempotente — a thread descarta `Play`
    /// quando o Sink já está tocando.
    pub fn start(&self) {
        let _ = self.tx.send(Cmd::Play);
    }

    /// Para imediatamente o Sink e libera o recurso.
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

/// Loop da thread dedicada de áudio.
///
/// Mantém um `Option<Sink>` como estado: `Some` = tocando, `None` =
/// parado. `Cmd::Play` cria/anexa o decoder em loop infinito;
/// `Cmd::Stop` para e descarta o Sink. O decoder MP3 é instanciado a
/// cada `Play` (não pode ser compartilhado entre Sinks) — custo
/// negligenciável porque acontece poucas vezes por sessão.
fn run_audio_thread(rx: mpsc::Receiver<Cmd>) {
    let (_stream, handle) = match OutputStream::try_default() {
        Ok(pair) => pair,
        Err(e) => {
            tracing::warn!("alarme: device de áudio indisponível ({e}); modal sem som");
            drain_channel(rx);
            return;
        }
    };
    let mut sink: Option<Sink> = None;
    loop {
        match rx.recv() {
            Ok(Cmd::Play) => {
                if sink.is_some() { continue; }
                sink = create_alarm_sink(&handle);
            }
            Ok(Cmd::Stop) => {
                if let Some(s) = sink.take() { s.stop(); }
            }
            Err(_) => break, // canal fechado → encerrar thread
        }
    }
}

/// Cria um `Sink` com o som do alarme em loop infinito. `Decoder::
/// new_looped` faz o rewind automático no fim do arquivo, então não
/// precisamos de `.repeat_infinite()`/`Cmd::Tick` periódico.
///
/// Retorna `None` se a rodio não conseguir criar o Sink OU decodificar
/// o MP3 — ambos os casos só acontecem por config inválida no boot
/// (device sumiu, asset corrompido) e são logados como warn.
fn create_alarm_sink(handle: &rodio::OutputStreamHandle) -> Option<Sink> {
    let sink = Sink::try_new(handle)
        .inspect_err(|e| tracing::warn!("alarme: falha ao criar Sink: {e}"))
        .ok()?;
    let cursor = Cursor::new(ALARM_BYTES);
    match Decoder::new_looped(cursor) {
        Ok(source) => {
            sink.append(source);
            Some(sink)
        }
        Err(e) => {
            tracing::warn!("alarme: falha ao decodificar alarm.mp3: {e}");
            None
        }
    }
}

/// Drena o canal silenciosamente quando o device de áudio falhou —
/// evita que `tx.send()` no caller bloqueie sem consumidor.
fn drain_channel(rx: mpsc::Receiver<Cmd>) {
    while rx.recv().is_ok() {}
}
