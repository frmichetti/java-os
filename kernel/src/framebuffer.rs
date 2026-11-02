//! Console de framebuffer — ver `design.md` da change
//! `kernel-framebuffer-console`.

use limine::framebuffer::{FRAMEBUFFER_RGB, Framebuffer};

use crate::sync::Spinlock;

/// Layout de pixel reportado pelo framebuffer (ver `limine::Framebuffer`):
/// o tamanho e deslocamento, em bits, de cada canal de cor dentro de um
/// pixel. Genérico o suficiente para cobrir tanto o caso comum (32 bpp,
/// 8 bits por canal) quanto variações (ex.: 16 bpp, 5-6-5) — ver design.md,
/// "Conversão de pixel".
#[derive(Clone, Copy)]
pub struct PixelFormat {
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

/// Trunca um canal de cor de 8 bits para `mask_size` bits (descartando os
/// bits menos significativos) e desloca o resultado para `mask_shift`.
/// `mask_size == 0` produz sempre `0` (canal ausente nesse formato de
/// pixel). Função pura, testável sem hardware.
fn pack_channel(value: u8, mask_size: u8, mask_shift: u8) -> u32 {
    if mask_size == 0 {
        return 0;
    }
    let truncated = (value as u32) >> (8u8.saturating_sub(mask_size));
    truncated << mask_shift
}

impl PixelFormat {
    /// Monta o valor de pixel (sem levar em conta `bpp` — ver
    /// `FramebufferConsole`, que decide quantos bytes desse valor escrever)
    /// a partir de um canal de cor de 8 bits por componente.
    pub fn pack(&self, r: u8, g: u8, b: u8) -> u32 {
        pack_channel(r, self.red_mask_size, self.red_mask_shift)
            | pack_channel(g, self.green_mask_size, self.green_mask_shift)
            | pack_channel(b, self.blue_mask_size, self.blue_mask_shift)
    }
}

/// Fonte bitmap 8×8 monocromática, autoral (não extraída de nenhum arquivo
/// externo nem de nenhuma fonte de terceiros — ver design.md, "Fonte
/// bitmap"), cobrindo o intervalo ASCII imprimível 0x20 (espaço) a 0x7E
/// (`~`). Cada glifo é uma linha de 8 bytes, um por linha de pixels; dentro
/// de cada byte, o bit mais significativo é a coluna mais à esquerda. A
/// maioria dos glifos usa um desenho em grade 5×7 centralizado nas colunas
/// 1-5 (bits 6-2), deixando a coluna 0 e as colunas 6-7 como espaçamento
/// entre caracteres, e a linha 7 livre para descendentes (`g`, `j`, `p`,
/// `q`, `y`).
#[rustfmt::skip]
const FONT: [[u8; 8]; 95] = [
    [0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00], // 0x20 ' '
    [0x20,0x20,0x20,0x20,0x20,0x00,0x20,0x00], // 0x21 '!'
    [0x28,0x28,0x00,0x00,0x00,0x00,0x00,0x00], // 0x22 '"'
    [0x28,0x28,0x7C,0x28,0x7C,0x28,0x28,0x00], // 0x23 '#'
    [0x10,0x3C,0x50,0x38,0x14,0x78,0x10,0x00], // 0x24 '$'
    [0x64,0x68,0x08,0x10,0x20,0x2C,0x4C,0x00], // 0x25 '%'
    [0x30,0x48,0x48,0x30,0x48,0x4C,0x34,0x00], // 0x26 '&'
    [0x20,0x20,0x40,0x00,0x00,0x00,0x00,0x00], // 0x27 '\''
    [0x10,0x20,0x40,0x40,0x40,0x20,0x10,0x00], // 0x28 '('
    [0x10,0x08,0x04,0x04,0x04,0x08,0x10,0x00], // 0x29 ')'
    [0x00,0x54,0x38,0x7C,0x38,0x54,0x00,0x00], // 0x2A '*'
    [0x00,0x10,0x10,0x7C,0x10,0x10,0x00,0x00], // 0x2B '+'
    [0x00,0x00,0x00,0x00,0x00,0x10,0x10,0x20], // 0x2C ','
    [0x00,0x00,0x00,0x7C,0x00,0x00,0x00,0x00], // 0x2D '-'
    [0x00,0x00,0x00,0x00,0x00,0x00,0x10,0x00], // 0x2E '.'
    [0x04,0x08,0x08,0x10,0x20,0x20,0x40,0x00], // 0x2F '/'
    [0x38,0x44,0x4C,0x54,0x64,0x44,0x38,0x00], // 0x30 '0'
    [0x10,0x30,0x10,0x10,0x10,0x10,0x38,0x00], // 0x31 '1'
    [0x38,0x44,0x04,0x08,0x10,0x20,0x7C,0x00], // 0x32 '2'
    [0x38,0x44,0x04,0x18,0x04,0x44,0x38,0x00], // 0x33 '3'
    [0x08,0x18,0x28,0x48,0x7C,0x08,0x08,0x00], // 0x34 '4'
    [0x7C,0x40,0x78,0x04,0x04,0x44,0x38,0x00], // 0x35 '5'
    [0x18,0x20,0x40,0x78,0x44,0x44,0x38,0x00], // 0x36 '6'
    [0x7C,0x04,0x08,0x10,0x10,0x10,0x10,0x00], // 0x37 '7'
    [0x38,0x44,0x44,0x38,0x44,0x44,0x38,0x00], // 0x38 '8'
    [0x38,0x44,0x44,0x3C,0x04,0x08,0x30,0x00], // 0x39 '9'
    [0x00,0x10,0x00,0x00,0x00,0x10,0x00,0x00], // 0x3A ':'
    [0x00,0x10,0x00,0x00,0x00,0x10,0x10,0x20], // 0x3B ';'
    [0x08,0x10,0x20,0x40,0x20,0x10,0x08,0x00], // 0x3C '<'
    [0x00,0x00,0x7C,0x00,0x7C,0x00,0x00,0x00], // 0x3D '='
    [0x20,0x10,0x08,0x04,0x08,0x10,0x20,0x00], // 0x3E '>'
    [0x38,0x44,0x04,0x08,0x10,0x00,0x10,0x00], // 0x3F '?'
    [0x38,0x44,0x5C,0x54,0x5C,0x40,0x38,0x00], // 0x40 '@'
    [0x10,0x28,0x44,0x44,0x7C,0x44,0x44,0x00], // 0x41 'A'
    [0x78,0x44,0x44,0x78,0x44,0x44,0x78,0x00], // 0x42 'B'
    [0x3C,0x40,0x40,0x40,0x40,0x40,0x3C,0x00], // 0x43 'C'
    [0x78,0x44,0x44,0x44,0x44,0x44,0x78,0x00], // 0x44 'D'
    [0x7C,0x40,0x40,0x78,0x40,0x40,0x7C,0x00], // 0x45 'E'
    [0x7C,0x40,0x40,0x78,0x40,0x40,0x40,0x00], // 0x46 'F'
    [0x3C,0x40,0x40,0x5C,0x44,0x44,0x3C,0x00], // 0x47 'G'
    [0x44,0x44,0x44,0x7C,0x44,0x44,0x44,0x00], // 0x48 'H'
    [0x38,0x10,0x10,0x10,0x10,0x10,0x38,0x00], // 0x49 'I'
    [0x0C,0x04,0x04,0x04,0x44,0x44,0x38,0x00], // 0x4A 'J'
    [0x44,0x48,0x50,0x60,0x50,0x48,0x44,0x00], // 0x4B 'K'
    [0x40,0x40,0x40,0x40,0x40,0x40,0x7C,0x00], // 0x4C 'L'
    [0x44,0x6C,0x54,0x44,0x44,0x44,0x44,0x00], // 0x4D 'M'
    [0x44,0x64,0x54,0x4C,0x44,0x44,0x44,0x00], // 0x4E 'N'
    [0x38,0x44,0x44,0x44,0x44,0x44,0x38,0x00], // 0x4F 'O'
    [0x78,0x44,0x44,0x78,0x40,0x40,0x40,0x00], // 0x50 'P'
    [0x38,0x44,0x44,0x44,0x54,0x48,0x34,0x00], // 0x51 'Q'
    [0x78,0x44,0x44,0x78,0x50,0x48,0x44,0x00], // 0x52 'R'
    [0x3C,0x40,0x40,0x38,0x04,0x04,0x78,0x00], // 0x53 'S'
    [0x7C,0x10,0x10,0x10,0x10,0x10,0x10,0x00], // 0x54 'T'
    [0x44,0x44,0x44,0x44,0x44,0x44,0x38,0x00], // 0x55 'U'
    [0x44,0x44,0x44,0x44,0x44,0x28,0x10,0x00], // 0x56 'V'
    [0x44,0x44,0x44,0x54,0x54,0x6C,0x44,0x00], // 0x57 'W'
    [0x44,0x28,0x10,0x10,0x10,0x28,0x44,0x00], // 0x58 'X'
    [0x44,0x28,0x10,0x10,0x10,0x10,0x10,0x00], // 0x59 'Y'
    [0x7C,0x04,0x08,0x10,0x20,0x40,0x7C,0x00], // 0x5A 'Z'
    [0x38,0x20,0x20,0x20,0x20,0x20,0x38,0x00], // 0x5B '['
    [0x40,0x20,0x20,0x10,0x08,0x08,0x04,0x00], // 0x5C '\\'
    [0x38,0x08,0x08,0x08,0x08,0x08,0x38,0x00], // 0x5D ']'
    [0x10,0x28,0x44,0x00,0x00,0x00,0x00,0x00], // 0x5E '^'
    [0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x7C], // 0x5F '_'
    [0x20,0x10,0x00,0x00,0x00,0x00,0x00,0x00], // 0x60 '`'
    [0x00,0x00,0x38,0x04,0x3C,0x44,0x3C,0x00], // 0x61 'a'
    [0x40,0x40,0x78,0x44,0x44,0x44,0x78,0x00], // 0x62 'b'
    [0x00,0x00,0x3C,0x40,0x40,0x40,0x3C,0x00], // 0x63 'c'
    [0x04,0x04,0x3C,0x44,0x44,0x44,0x3C,0x00], // 0x64 'd'
    [0x00,0x00,0x38,0x44,0x7C,0x40,0x38,0x00], // 0x65 'e'
    [0x18,0x20,0x78,0x20,0x20,0x20,0x20,0x00], // 0x66 'f'
    [0x00,0x00,0x3C,0x44,0x44,0x3C,0x04,0x38], // 0x67 'g'
    [0x40,0x40,0x78,0x44,0x44,0x44,0x44,0x00], // 0x68 'h'
    [0x10,0x00,0x30,0x10,0x10,0x10,0x38,0x00], // 0x69 'i'
    [0x08,0x00,0x18,0x08,0x08,0x08,0x48,0x30], // 0x6A 'j'
    [0x40,0x40,0x48,0x50,0x60,0x50,0x48,0x00], // 0x6B 'k'
    [0x30,0x10,0x10,0x10,0x10,0x10,0x38,0x00], // 0x6C 'l'
    [0x00,0x00,0x68,0x54,0x54,0x44,0x44,0x00], // 0x6D 'm'
    [0x00,0x00,0x78,0x44,0x44,0x44,0x44,0x00], // 0x6E 'n'
    [0x00,0x00,0x38,0x44,0x44,0x44,0x38,0x00], // 0x6F 'o'
    [0x00,0x00,0x78,0x44,0x44,0x78,0x40,0x40], // 0x70 'p'
    [0x00,0x00,0x3C,0x44,0x44,0x3C,0x04,0x04], // 0x71 'q'
    [0x00,0x00,0x58,0x64,0x40,0x40,0x40,0x00], // 0x72 'r'
    [0x00,0x00,0x3C,0x40,0x38,0x04,0x78,0x00], // 0x73 's'
    [0x20,0x20,0x78,0x20,0x20,0x24,0x18,0x00], // 0x74 't'
    [0x00,0x00,0x44,0x44,0x44,0x4C,0x34,0x00], // 0x75 'u'
    [0x00,0x00,0x44,0x44,0x44,0x28,0x10,0x00], // 0x76 'v'
    [0x00,0x00,0x44,0x44,0x54,0x54,0x28,0x00], // 0x77 'w'
    [0x00,0x00,0x44,0x28,0x10,0x28,0x44,0x00], // 0x78 'x'
    [0x00,0x00,0x44,0x44,0x44,0x3C,0x04,0x38], // 0x79 'y'
    [0x00,0x00,0x7C,0x08,0x10,0x20,0x7C,0x00], // 0x7A 'z'
    [0x18,0x20,0x20,0x40,0x20,0x20,0x18,0x00], // 0x7B '{'
    [0x10,0x10,0x10,0x10,0x10,0x10,0x10,0x00], // 0x7C '|'
    [0x60,0x10,0x10,0x08,0x10,0x10,0x60,0x00], // 0x7D '}'
    [0x00,0x00,0x24,0x58,0x00,0x00,0x00,0x00], // 0x7E '~'
];

/// Glifo de reserva (um retângulo oco) para qualquer caractere fora do
/// intervalo coberto pela fonte — nunca memória não inicializada nem um
/// glifo aleatório.
const FALLBACK_GLYPH: [u8; 8] = [0x7C, 0x44, 0x44, 0x44, 0x44, 0x44, 0x7C, 0x00];

/// Devolve o glifo 8×8 de `c`, ou [`FALLBACK_GLYPH`] se `c` estiver fora do
/// intervalo ASCII imprimível coberto pela fonte (0x20 a 0x7E).
pub fn glyph_for(c: char) -> &'static [u8; 8] {
    let code = c as u32;
    if (0x20..=0x7E).contains(&code) {
        &FONT[(code - 0x20) as usize]
    } else {
        &FALLBACK_GLYPH
    }
}

/// Posição lógica do cursor de texto (linha/coluna, em caracteres — não em
/// pixels) dentro de um console de `cols` colunas por `rows` linhas. Lógica
/// pura, sem nenhuma dependência de framebuffer real — ver design.md,
/// "Rolagem".
pub struct CursorPosition {
    pub col: usize,
    pub row: usize,
    cols: usize,
    rows: usize,
}

impl CursorPosition {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self { col: 0, row: 0, cols, rows }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Avança uma coluna; ao atingir o fim da largura, quebra para o início
    /// da próxima linha (mesma semântica de sinalização de rolagem de
    /// [`Self::newline`]).
    ///
    /// Devolve `true` exatamente quando essa quebra ultrapassaria a última
    /// linha — nesse caso uma rolagem é necessária, e o cursor permanece na
    /// própria última linha (coluna 0), pronto para receber o texto depois
    /// que o chamador realizar a rolagem.
    pub fn advance(&mut self) -> bool {
        self.col += 1;
        if self.col >= self.cols {
            self.newline()
        } else {
            false
        }
    }

    /// Quebra explicitamente de linha (ex.: ao encontrar `'\n'`), com a
    /// mesma semântica de sinalização de rolagem de [`Self::advance`].
    pub fn newline(&mut self) -> bool {
        self.col = 0;
        if self.row + 1 >= self.rows {
            true
        } else {
            self.row += 1;
            false
        }
    }
}

/// Cor de primeiro plano (texto) e de fundo fixas — ver design.md,
/// Non-Goals ("sem customização de cor além de um par fixo de cor de
/// primeiro plano/fundo").
const FOREGROUND: (u8, u8, u8) = (0xC0, 0xC0, 0xC0);
const BACKGROUND: (u8, u8, u8) = (0x00, 0x00, 0x00);

/// Largura/altura, em pixels, de cada glifo — e, portanto, de cada célula
/// de caractere desenhada na tela.
const GLYPH_WIDTH: usize = 8;
const GLYPH_HEIGHT: usize = 8;

/// Driver de console de texto sobre o framebuffer linear fornecido pelo
/// Limine — ver design.md. Guarda o ponteiro base (já utilizável, ver
/// design.md "Inicialização cedo") e o layout reportado pelo bootloader,
/// mais a posição lógica do cursor (tarefa 2.4).
pub struct FramebufferConsole {
    base: *mut u8,
    width: usize,
    pitch: usize,
    bytes_per_pixel: usize,
    format: PixelFormat,
    cursor: CursorPosition,
}

// `base` é um ponteiro bruto para a memória do framebuffer: o kernel ainda
// não é SMP (nenhum compartilhamento real entre núcleos) e todo acesso já
// passa pelo `Spinlock` de `CONSOLE` — seguro marcar como `Send`.
unsafe impl Send for FramebufferConsole {}

impl FramebufferConsole {
    fn new(
        base: *mut u8,
        width: usize,
        height: usize,
        pitch: usize,
        bytes_per_pixel: usize,
        format: PixelFormat,
    ) -> Self {
        let cols = width / GLYPH_WIDTH;
        let rows = height / GLYPH_HEIGHT;
        Self {
            base,
            width,
            pitch,
            bytes_per_pixel,
            format,
            cursor: CursorPosition::new(cols, rows),
        }
    }

    /// Escreve um pixel já convertido (ver [`PixelFormat::pack`]) na posição
    /// `(x, y)`, em pixels, respeitando `bytes_per_pixel` (2, 3 ou 4 — ver
    /// [`FramebufferError`]).
    fn put_pixel(&mut self, x: usize, y: usize, value: u32) {
        let offset = y * self.pitch + x * self.bytes_per_pixel;
        let bytes = value.to_le_bytes();
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), self.base.add(offset), self.bytes_per_pixel);
        }
    }

    /// Desenha o glifo de `c` (tarefa 2.3) na célula de caractere
    /// `(col, row)` — em coordenadas de caractere, não de pixel.
    fn draw_glyph(&mut self, col: usize, row: usize, c: char) {
        let glyph = *glyph_for(c);
        let fg = self.format.pack(FOREGROUND.0, FOREGROUND.1, FOREGROUND.2);
        let bg = self.format.pack(BACKGROUND.0, BACKGROUND.1, BACKGROUND.2);
        let base_x = col * GLYPH_WIDTH;
        let base_y = row * GLYPH_HEIGHT;
        for (dy, line) in glyph.iter().enumerate() {
            for dx in 0..GLYPH_WIDTH {
                let bit = (line >> (7 - dx)) & 1;
                let value = if bit != 0 { fg } else { bg };
                self.put_pixel(base_x + dx, base_y + dy, value);
            }
        }
    }

    /// Limpa (preenche com a cor de fundo) uma linha de texto inteira, em
    /// coordenadas de caractere.
    fn clear_row(&mut self, row: usize) {
        let bg = self.format.pack(BACKGROUND.0, BACKGROUND.1, BACKGROUND.2);
        let base_y = row * GLYPH_HEIGHT;
        for dy in 0..GLYPH_HEIGHT {
            for x in 0..self.width {
                self.put_pixel(x, base_y + dy, bg);
            }
        }
    }

    /// Rola o conteúdo uma linha de texto para cima e limpa a última linha
    /// — ver design.md, "Rolagem": copia os pixels de todas as linhas de
    /// texto a partir da segunda para uma posição uma linha de texto acima,
    /// via `core::ptr::copy` (as regiões se sobrepõem), e limpa a última
    /// linha de texto com a cor de fundo.
    fn scroll(&mut self) {
        let rows = self.cursor.rows();
        let copy_len = (rows - 1) * GLYPH_HEIGHT * self.pitch;
        unsafe {
            core::ptr::copy(self.base.add(GLYPH_HEIGHT * self.pitch), self.base, copy_len);
        }
        self.clear_row(rows - 1);
    }

    fn handle_scroll_signal(&mut self, needs_scroll: bool) {
        if needs_scroll {
            self.scroll();
        }
    }
}

impl core::fmt::Write for FramebufferConsole {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            if c == '\n' {
                let needs_scroll = self.cursor.newline();
                self.handle_scroll_signal(needs_scroll);
                continue;
            }
            let (col, row) = (self.cursor.col, self.cursor.row);
            self.draw_glyph(col, row, c);
            let needs_scroll = self.cursor.advance();
            self.handle_scroll_signal(needs_scroll);
        }
        Ok(())
    }
}

impl crate::hal::Console for FramebufferConsole {}

/// Motivo pelo qual um framebuffer reportado pelo Limine não pôde ser
/// utilizado — ver design.md, Non-Goals ("só `memory_model == FRAMEBUFFER_RGB`
/// com 16/24/32 bits por pixel; qualquer outra coisa é tratada como 'sem
/// framebuffer utilizável'").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramebufferError {
    /// `memory_model` reportado não é `FRAMEBUFFER_RGB` (único suportado).
    UnsupportedMemoryModel(u8),
    /// `bpp` reportado não é 16, 24 nem 32.
    UnsupportedBpp(u16),
}

/// Valida o modelo de memória e os bits por pixel reportados pelo Limine,
/// devolvendo quantos bytes ocupa cada pixel quando aceitos. Lógica pura,
/// sem tocar memória real de framebuffer — testável em host.
fn validate_format(memory_model: u8, bpp: u16) -> Result<usize, FramebufferError> {
    if memory_model != FRAMEBUFFER_RGB {
        return Err(FramebufferError::UnsupportedMemoryModel(memory_model));
    }
    match bpp {
        16 => Ok(2),
        24 => Ok(3),
        32 => Ok(4),
        other => Err(FramebufferError::UnsupportedBpp(other)),
    }
}

/// Console de framebuffer global, inicializado por [`init`] — `None` antes
/// da inicialização ou se nenhum framebuffer utilizável foi encontrado.
/// Mesmo padrão de `Spinlock` já usado por `kernel::serial::SERIAL1` (ver
/// design.md, "Riscos/Trade-offs").
static CONSOLE: Spinlock<Option<FramebufferConsole>> = Spinlock::new(None);

/// Inicializa o console de framebuffer global a partir de um `Framebuffer`
/// reportado pelo Limine, validando `memory_model`/`bpp` (ver
/// [`validate_format`]) antes de desenhar qualquer pixel. Deve ser chamada
/// no máximo uma vez, cedo em `kernel_main` (ver design.md, "Inicialização
/// cedo").
pub fn init(fb: &Framebuffer) -> Result<(), FramebufferError> {
    let bytes_per_pixel = validate_format(fb.memory_model, fb.bpp)?;
    let format = PixelFormat {
        red_mask_size: fb.red_mask_size,
        red_mask_shift: fb.red_mask_shift,
        green_mask_size: fb.green_mask_size,
        green_mask_shift: fb.green_mask_shift,
        blue_mask_size: fb.blue_mask_size,
        blue_mask_shift: fb.blue_mask_shift,
    };
    let console = FramebufferConsole::new(
        fb.address().cast::<u8>(),
        fb.width as usize,
        fb.height as usize,
        fb.pitch as usize,
        bytes_per_pixel,
        format,
    );
    *CONSOLE.lock() = Some(console);
    Ok(())
}

/// Escreve no console de framebuffer global, se já inicializado; não faz
/// nada caso contrário (ainda cedo no boot, ou framebuffer indisponível) —
/// ver design.md, "Espelhamento". Chamada por `kernel::serial::_print`.
pub fn mirror(args: core::fmt::Arguments) {
    if let Some(console) = CONSOLE.lock().as_mut() {
        let _ = core::fmt::Write::write_fmt(console, args);
    }
}

/// Igual a [`mirror`], mas para quem já tem um `&str` pronto em vez de
/// `fmt::Arguments` — usada por código que não passa pelas macros
/// `serial_println!`/`serial_print!` (ex.: `kernel::serial::MirroredWriter`,
/// a syscall de escrita), mas cujo conteúdo o design.md desta change
/// também lista como "código de diagnóstico já existente" a ser espelhado
/// (mapa de memória, saída de programas de usuário via syscall).
pub fn mirror_str(s: &str) {
    if let Some(console) = CONSOLE.lock().as_mut() {
        let _ = core::fmt::Write::write_str(console, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RGB888: PixelFormat = PixelFormat {
        red_mask_size: 8,
        red_mask_shift: 16,
        green_mask_size: 8,
        green_mask_shift: 8,
        blue_mask_size: 8,
        blue_mask_shift: 0,
    };

    #[test]
    fn packs_rgb888_in_the_expected_bit_positions() {
        assert_eq!(RGB888.pack(0xff, 0x00, 0x00), 0x00ff_0000);
        assert_eq!(RGB888.pack(0x00, 0xff, 0x00), 0x0000_ff00);
        assert_eq!(RGB888.pack(0x00, 0x00, 0xff), 0x0000_00ff);
        assert_eq!(RGB888.pack(0x12, 0x34, 0x56), 0x0012_3456);
    }

    #[test]
    fn truncates_a_channel_to_a_smaller_mask_keeping_the_most_significant_bits() {
        // RGB565: vermelho com 5 bits, no deslocamento 11.
        let value = pack_channel(0xff, 5, 11);
        assert_eq!(value, 0b11111 << 11, "deve manter os 5 bits mais significativos de 0xff");

        let value = pack_channel(0b0000_0111, 5, 0);
        assert_eq!(value, 0, "os 5 bits mais significativos de 0b0000_0111 são todos zero");
    }

    #[test]
    fn zero_mask_size_produces_zero_regardless_of_value() {
        assert_eq!(pack_channel(0xff, 0, 4), 0);
    }

    #[test]
    fn characters_in_range_return_a_non_empty_glyph_except_space() {
        for code in 0x21u32..=0x7E {
            let c = char::from_u32(code).unwrap();
            let glyph = glyph_for(c);
            assert!(
                glyph.iter().any(|&row| row != 0),
                "glifo de '{c}' (0x{code:02X}) nao deveria estar vazio"
            );
        }

        assert_eq!(*glyph_for(' '), [0u8; 8], "o espaco deve ser o unico glifo legitimamente vazio");
    }

    #[test]
    fn characters_outside_the_covered_range_return_the_fallback_glyph_without_out_of_bounds_access() {
        // Caractere acentuado (fora de 0x20..=0x7E): deve devolver o glifo de
        // reserva, nunca indexar fora da tabela `FONT`.
        assert_eq!(*glyph_for('é'), FALLBACK_GLYPH);
        // Limites exatos ao redor do intervalo coberto, para garantir que a
        // checagem de limites está correta (nem um a menos, nem um a mais).
        assert_eq!(*glyph_for('\u{1F}'), FALLBACK_GLYPH);
        assert_eq!(*glyph_for('\u{7F}'), FALLBACK_GLYPH);
        // Caractere de controle unicode bem distante, para garantir que
        // nenhuma conversão aritmética estoura os limites da tabela.
        assert_eq!(*glyph_for('\u{1F600}'), FALLBACK_GLYPH);
    }

    #[test]
    fn advance_moves_one_column_without_touching_the_row_before_the_end_of_width() {
        let mut cursor = CursorPosition::new(80, 25);
        assert!(!cursor.advance());
        assert_eq!((cursor.col, cursor.row), (1, 0));
        assert!(!cursor.advance());
        assert_eq!((cursor.col, cursor.row), (2, 0));
    }

    #[test]
    fn advance_wraps_to_the_next_line_at_the_end_of_width() {
        let mut cursor = CursorPosition::new(3, 25);
        assert!(!cursor.advance()); // col 0 -> 1
        assert!(!cursor.advance()); // col 1 -> 2 (ultima coluna valida)
        assert!(!cursor.advance()); // col 2 -> quebra de linha
        assert_eq!((cursor.col, cursor.row), (0, 1), "deve quebrar para a proxima linha, nao ultrapassar a largura");
    }

    #[test]
    fn scroll_is_signaled_exactly_when_writing_past_the_last_line_never_before_never_after() {
        let mut cursor = CursorPosition::new(80, 2);
        // Ainda na primeira linha (linha 0 de 2): nenhuma rolagem.
        assert!(!cursor.newline(), "nao deve sinalizar rolagem antes de alcancar a ultima linha");
        assert_eq!(cursor.row, 1);
        // Agora na ultima linha (linha 1 de 2): quebrar aqui exige rolagem.
        assert!(cursor.newline(), "deve sinalizar rolagem exatamente ao ultrapassar a ultima linha");
        // O cursor permanece na ultima linha, pronto para o texto pos-rolagem.
        assert_eq!((cursor.col, cursor.row), (0, 1), "apos sinalizar rolagem, o cursor fica na ultima linha, nao alem dela");
        // Rolando novamente a partir da ultima linha continua sinalizando,
        // nunca saindo dos limites.
        assert!(cursor.newline());
        assert_eq!((cursor.col, cursor.row), (0, 1));
    }

    #[test]
    fn accepts_rgb_memory_model_with_each_supported_bpp() {
        assert_eq!(validate_format(FRAMEBUFFER_RGB, 16), Ok(2));
        assert_eq!(validate_format(FRAMEBUFFER_RGB, 24), Ok(3));
        assert_eq!(validate_format(FRAMEBUFFER_RGB, 32), Ok(4));
    }

    #[test]
    fn rejects_any_memory_model_other_than_rgb_regardless_of_bpp() {
        let other_model = FRAMEBUFFER_RGB.wrapping_add(1);
        assert_eq!(
            validate_format(other_model, 32),
            Err(FramebufferError::UnsupportedMemoryModel(other_model))
        );
    }

    #[test]
    fn rejects_any_bpp_outside_the_supported_set() {
        for bpp in [0u16, 8, 15, 17, 23, 25, 31, 33, 64] {
            assert_eq!(
                validate_format(FRAMEBUFFER_RGB, bpp),
                Err(FramebufferError::UnsupportedBpp(bpp)),
                "bpp={bpp} deveria ser rejeitado"
            );
        }
    }
}
