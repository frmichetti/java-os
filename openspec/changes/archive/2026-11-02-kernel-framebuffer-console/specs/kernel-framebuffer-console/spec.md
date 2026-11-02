# Spec Delta

## Purpose

Fornece ao kernel um console de texto visível numa tela, renderizado no framebuffer linear que o bootloader já disponibiliza, como canal de diagnóstico adicional ao console serial existente.

## ADDED Requirements

### Requirement: Texto Renderizado no Framebuffer
O kernel SHALL renderizar texto no framebuffer linear fornecido pelo bootloader, usando uma fonte bitmap própria, sem depender de nenhum arquivo externo carregado de disco.

#### Scenario: Texto escrito aparece como pixels no framebuffer
- **WHEN** uma linha de texto é escrita no console de framebuffer
- **THEN** os caracteres dessa linha SHALL aparecer desenhados como pixels na região do framebuffer correspondente à linha de texto atual

### Requirement: Rolagem Quando a Tela Enche
Quando o texto escrito ultrapassar a última linha visível do framebuffer, o console SHALL rolar o conteúdo já exibido para cima, continuando a exibir o texto mais recente, em vez de parar de exibir novas linhas ou escrever fora da área visível.

#### Scenario: Escrever além da última linha rola o conteúdo anterior
- **WHEN** uma nova linha é escrita depois que todas as linhas visíveis do framebuffer já estão ocupadas
- **THEN** o conteúdo já exibido SHALL rolar uma linha para cima, e a nova linha SHALL aparecer na última posição, em vez de a escrita falhar ou corromper a área visível

### Requirement: Mensagens de Diagnóstico Refletidas no Framebuffer
A partir do momento em que o console de framebuffer é inicializado, as mesmas mensagens de diagnóstico de boot já emitidas no console serial SHALL também aparecer no framebuffer.

#### Scenario: Mensagem de diagnóstico emitida depois da inicialização aparece em ambos os canais
- **WHEN** uma mensagem de diagnóstico de boot é emitida depois que o console de framebuffer já foi inicializado
- **THEN** essa mensagem SHALL aparecer tanto no console serial quanto no console de framebuffer

### Requirement: Indisponibilidade do Framebuffer é Tratada Explicitamente
Caso o bootloader não forneça uma resposta de framebuffer utilizável, o kernel SHALL reportar essa ausência de forma explícita pelo canal de saída já disponível (o console serial) e continuar o boot normalmente, em vez de travar ou falhar silenciosamente.

#### Scenario: Ausência de framebuffer não impede o boot
- **WHEN** o bootloader não responde à requisição de framebuffer, ou responde sem nenhum framebuffer utilizável
- **THEN** o kernel SHALL reportar essa ausência no console serial e prosseguir o boot normalmente, sem o console de framebuffer
