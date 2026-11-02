# Design

## Context

Hoje `kernel::serial` é o único canal de saída: um `SerialPort` (UART 16550) implementando `core::fmt::Write`, acessado via as macros `serial_println!`/`serial_print!` (e as variantes `_emergency`, que ignoram o lock para handlers de falha irrecuperável). `kernel_main` chama `serial::init()` bem cedo (logo após GDT/IDT), antes mesmo de `HHDM_REQUEST`/`hhdm::init()` serem processados. A crate `limine` (0.6.5, já uma dependência) expõe `FramebufferRequest`/`Framebuffer` (campos `address()`, `width`, `height`, `pitch`, `bpp`, `memory_model`, `red/green/blue_mask_size/shift` — `FRAMEBUFFER_RGB = 1` é o único modelo documentado), ainda não declarada pelo kernel. Não existe HAL alguma ainda — cada driver (serial, timer/APIC) implementa sua própria interface ad-hoc. Ver `proposal.md` — Why/What Changes para a motivação; `specs/kernel-hal` e `specs/kernel-framebuffer-console` para os requisitos.

## Goals / Non-Goals

**Goals:**
- Um trait `Console` mínimo, implementado tanto pelo console serial já existente quanto pelo novo console de framebuffer.
- Um console de texto funcional sobre o framebuffer linear do Limine: fonte bitmap própria, rolagem de linhas.
- Mensagens de diagnóstico de boot refletidas em ambos os canais a partir da inicialização do framebuffer, idealmente desde o próprio banner.
- Tratamento explícito (sem travar, sem fabricar dados) de um framebuffer indisponível.

**Non-Goals (nesta change):**
- Teclado, armazenamento, rede — próximos drivers da mesma área do roadmap, cada um como change futura.
- Qualquer gráfico além de texto: sem formas, imagens, múltiplas janelas, cursor posicionável, ou customização de cor além de um par fixo de cor de primeiro plano/fundo.
- Double buffering ou sincronização com retraço de vídeo — escrita direta nos pixels do framebuffer é aceitável para um log de boot (não há "jogo" ou animação a proteger de *tearing*).
- Um broker/registro genérico de múltiplos consoles arbitrários — só dois consoles existem (serial, framebuffer); uma abstração mais genérica fica para quando houver um terceiro consumidor real.
- Suporte a todo e qualquer formato de pixel do Limine — só `memory_model == FRAMEBUFFER_RGB` com 16/24/32 bits por pixel; qualquer outra coisa é tratada como "sem framebuffer utilizável" (mesmo caminho de indisponibilidade).

## Decisions

### `Console`: trait marcador sobre `core::fmt::Write`
```rust
pub trait Console: core::fmt::Write {}
```
Não precisa de métodos próprios nesta primeira versão — `core::fmt::Write` já dá exatamente o que as specs exigem (escrever texto). `SerialPort` e o novo `FramebufferConsole` implementam os dois; `Console` existe como o nome que a HAL usa para se referir a "qualquer coisa que aceita texto", preparando o terreno para drivers futuros (teclado, armazenamento) definirem seus próprios traits (`KeyboardDevice`, `BlockDevice`) no mesmo módulo `kernel::hal`, sem precisar de nenhum método extra em `Console` agora.

**Alternativa descartada**: um trait `Console` com métodos próprios (`fn write_line`, `fn clear`, etc.), espelhando o que bibliotecas de console mais completas costumam ter. Rejeitada por antecipar necessidades que não existem ainda (Non-Goal: sem cursor, sem cor customizável) — `core::fmt::Write` já cobre tudo que é exigido pelas specs desta change.

### Conversão de pixel: genérica a partir das máscaras RGB, não hardcoded para 32 bpp
Em vez de assumir um layout de pixel fixo (ex.: sempre 0xRRGGBB em 32 bits), a conversão de uma cor lógica (r, g, b de 8 bits cada) para o valor de pixel usa `red/green/blue_mask_size`/`_shift` do `Framebuffer` reportado pelo Limine: cada canal de 8 bits é truncado para `mask_size` bits (descartando os bits menos significativos) e deslocado para `mask_shift`. O valor resultante é então escrito na memória com a largura correta (2, 3 ou 4 bytes, conforme `bpp`). Isso funciona tanto para o caso comum (32 bpp, 8 bits por canal) quanto para variações (ex.: 16 bpp, 5-6-5) sem precisar de um caminho de código separado para cada uma.

**Alternativa descartada**: assumir sempre 32 bpp RGB888 (o caso mais comum em QEMU/UEFI) e rejeitar qualquer outra coisa. Mais simples, mas despisa a informação que o próprio Limine já fornece (as máscaras) só para economizar uma função; o caminho genérico não é significativamente mais complexo e evita uma limitação arbitrária.

### Fonte bitmap: 8×8 monocromática, autoral, não extraída de nenhum arquivo externo
A fonte é uma tabela `const` no próprio código-fonte do kernel: 1 bit por pixel, 8 bytes por glifo (uma linha de 8 pixels por byte), cobrindo o intervalo ASCII imprimível (32 a 126). Um caractere fora desse intervalo é desenhado como um glifo de reserva (ex.: um retângulo preenchido), nunca como memória não inicializada ou um glifo aleatório. Por não haver sistema de arquivos ainda, a fonte precisa estar embutida no binário — e por ser escrita para este projeto especificamente (não copiada de uma fonte de terceiros), não há nenhuma questão de licença/atribuição a resolver.

**Alternativa descartada**: embutir uma fonte bitmap "padrão" de terceiros (ex.: a clássica fonte VGA 8×16 da IBM, amplamente reproduzida em tutoriais de OS dev). Rejeitada não por causa de licença (fontes bitmap de BIOS/VGA desse tipo são universalmente tratadas como de domínio público/livre reprodução na comunidade de OS dev), mas porque reproduzir os bytes exatos de memória, sem conferência externa, arrisca erros sutis de transcrição; uma fonte própria, mesmo mais simples, é uma superfície de erro menor e evita qualquer dúvida.

### Inicialização cedo, antes de HHDM — hipótese a confirmar em boot real
Diferente do heap/paginação (que precisam de HHDM), a hipótese de trabalho é que `Framebuffer::address()` já devolve um ponteiro diretamente utilizável (a própria Limine deixa o framebuffer mapeado antes de entregar o controle ao kernel), não um endereço físico cru como os `base` do mapa de memória. Se essa hipótese se confirmar, o console de framebuffer pode ser inicializado logo após `serial::init()`, **antes** de `HHDM_REQUEST`/`hhdm::init()` — permitindo que o banner de boot (a primeiríssima mensagem) já apareça na tela, não só a partir de algum ponto no meio do boot.

**Risco se a hipótese estiver errada**: o próprio boot real (QEMU) revela isso imediatamente — um ponteiro mal interpretado como "já mapeado" quando na verdade é físico cru causaria um page fault ao primeiro acesso de escrita (mesma classe de erro já vista em changes anteriores). Mitigação: validar isso logo na primeira tarefa de implementação que escreve de fato no framebuffer, antes de prosseguir; se a hipótese estiver errada, mover a inicialização do framebuffer para depois de `hhdm::init()` e converter `address()` (tratado então como físico) via `hhdm::phys_to_virt` — mudança pequena e localizada, não uma mudança de design.

### Espelhamento: `serial::_print` passa a também escrever no framebuffer, se inicializado
Em vez de um broker/registro genérico de consoles (Non-Goal, dado que só existem dois), `kernel::framebuffer` expõe uma função (`mirror(args: core::fmt::Arguments)`) que escreve no console de framebuffer se ele já estiver inicializado, e não faz nada caso contrário (ainda cedo no boot, ou o framebuffer indisponível). `serial::_print` (usada por `serial_println!`/`serial_print!`, já chamada em todo o código de diagnóstico existente) passa a chamar essa função depois de escrever no console serial. Isso espelha automaticamente **todo** o código de diagnóstico já existente (banner, mapa de memória, heap, APIC, timer, escalonador, syscalls) sem precisar tocar em nenhum call site individual — só a implementação de `_print` muda, não sua assinatura nem o nome das macros.

**Alternativa descartada**: introduzir uma macro nova (ex.: `console_println!`) e trocar os call sites relevantes em `kernel_main`/`continue_boot` para usá-la. Rejeitada por exigir tocar em vários pontos já estáveis do código de boot só para trocar o nome da macro usada, quando o objetivo (espelhar tudo, automaticamente) é mais simples de alcançar mudando a implementação de uma função já centralizada.

### Rolagem: copiar linhas de pixels para cima, mesma técnica de um terminal simples
Ao atingir a última linha de texto visível, rolar significa copiar o conteúdo de pixels da segunda linha de texto em diante para uma posição uma linha de texto acima (via `core::ptr::copy` sobre a região do framebuffer), e limpar (preencher com a cor de fundo) a última linha antes de desenhar o novo texto nela. Mantém o estado do console (posição do cursor lógico — linha/coluna atual) em uma struct simples, sem depender de nenhum buffer de texto fora do próprio framebuffer (o framebuffer *é* o estado visual).

## Achados durante a implementação

- **Hipótese sobre `Framebuffer::address()` confirmada**: boot real em QEMU (verificação visual via `screendump` do monitor QEMU, convertido de PPM para PNG) mostrou o texto desenhado corretamente desde o banner, sem nenhum page fault — o endereço já vem diretamente utilizável, como esperado. Nenhum ajuste de `hhdm::phys_to_virt` foi necessário.
- **Espelhamento via `_print` não cobria todo o código de diagnóstico existente, ao contrário do que a seção "Espelhamento" acima presumia**: `memmap_report::write_memory_map` (chamada em `main.rs` com `&mut *serial::SERIAL1.lock()`) e `syscall::sys_write` (escreve byte a byte via `SerialPort::write_byte`) escrevem diretamente na porta serial, sem passar pelas macros `serial_println!`/`serial_print!` nem por `serial::_print`. Isso só foi percebido na verificação visual da tarefa 5.1: o mapa de memória e a saída do programa de usuário via syscall simplesmente não apareciam na tela. Corrigido com duas adições pequenas, sem mudar a assinatura pública de nenhuma das duas funções:
  - `kernel::framebuffer::mirror_str(s: &str)`, irmã de `mirror(args: fmt::Arguments)`, para quem já tem um `&str` pronto.
  - `kernel::serial::MirroredWriter`, um adaptador de `core::fmt::Write` que escreve na serial e espelha via `mirror_str`, usado no lugar de `&mut *serial::SERIAL1.lock()` no call site de `write_memory_map` (que continua genérico sobre `W: Write`, portanto continua puro/testável em host sem nenhuma mudança).
  - `sys_write` passou a chamar `mirror_str` também, só quando os bytes recebidos são UTF-8 válido (o contrato da própria syscall, de escrever bytes crus na serial, não muda).
  Confirmado visualmente depois da correção: o mapa de memória aparece na tela (capturado via `screendump` pausando a CPU bem cedo no boot, antes do escalonador/demo de usuário começarem a produzir saída), e a saída repetida do programa de usuário ("ola do modo usuario, via syscall!") enche a tela e rola corretamente.

## Riscos / Trade-offs

- **[Risco] Hipótese sobre `Framebuffer::address()` já estar mapeado pode estar errada** → Mitigação já descrita acima (verificação imediata em boot real; correção é uma mudança pequena e localizada).
- **[Risco] Escrever diretamente nos pixels do framebuffer a partir de `_print` (potencialmente chamado por qualquer parte do kernel, inclusive em contexto de interrupção/emergência) pode ser lento ou, em teoria, ser interrompido a meio de uma operação não atômica** → Mitigação: o console de framebuffer usa o mesmo padrão de `Spinlock` já usado por `SERIAL1`, e a rolagem/desenho de um caractere são operações curtas o suficiente para não serem um problema prático nesta fase (sem SMP, sem seção crítica longa); a variante `_emergency` (usada por handlers de exceção) já ignora locks por design (ver `serial::_print_emergency`) — o framebuffer segue o mesmo princípio para não arriscar travar ao reportar uma falha fatal.
- **[Trade-off] Fonte bitmap própria, simples, em vez de uma fonte "bonita" de terceiros** → Aceitável: o objetivo desta change é provar o mecanismo (texto legível numa tela), não qualidade tipográfica; nada impede trocar a fonte depois sem mudar a interface do console.
- **[Trade-off] `kernel::serial` passa a depender de `kernel::framebuffer` (chamada em `_print`)** → Uma pequena inversão de dependência (o driver "mais antigo" conhece o "mais novo"), aceita em vez de um broker genérico (Non-Goal) porque só há dois consoles; revisar se um terceiro canal de diagnóstico aparecer no futuro.

## Migration Plan

Não há estado em produção. "Deploy" continua significando: o smoke test de boot (`cargo xtask test`) continua passando sem nenhuma mudança (ele só verifica o console serial, que permanece byte-a-byte idêntico a antes), e uma verificação manual (visual, via a janela de display do QEMU — `cargo xtask run` sem `-display none`) confirma que o banner e as mensagens de diagnóstico subsequentes aparecem corretamente na tela, incluindo pelo menos uma rolagem real (mais linhas de diagnóstico do que cabem na tela de uma vez). Rollback é reverter os commits desta change; nenhum dado persistente é afetado.
