# kernel-hal Specification

## Purpose

Fornece a primeira abstração da camada de HAL (hardware abstraction layer) do kernel: um trait para saída de texto, independente do dispositivo físico que a implementa, para que diagnósticos de boot (e drivers futuros) não precisem conhecer qual console específico está em uso.

## Requirements

### Requirement: Abstração de Console Independente de Dispositivo
O kernel SHALL fornecer uma interface comum para escrita de texto em um console, de forma que o mesmo código de diagnóstico funcione sobre qualquer dispositivo que implemente essa interface, sem conhecer os detalhes do dispositivo físico.

#### Scenario: Mesmo texto escrito através de dispositivos diferentes produz a mesma saída lógica
- **WHEN** a mesma sequência de texto é escrita através da interface comum em dois dispositivos diferentes que a implementam
- **THEN** ambos SHALL produzir a mesma saída lógica (o mesmo conteúdo textual), cada um em seu próprio formato de exibição
