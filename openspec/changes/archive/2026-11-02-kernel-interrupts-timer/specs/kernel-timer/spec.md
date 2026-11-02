# Spec Delta

## Purpose

Mantém uma fonte de tempo para o kernel via o timer do Local APIC, disparando interrupções periódicas e expondo um contador de ticks, base para o futuro escalonador preemptivo.

## ADDED Requirements

### Requirement: Timer Periódico via APIC
O kernel SHALL configurar o timer do Local APIC para gerar interrupções em intervalos regulares e previsíveis, a partir da inicialização do timer.

#### Scenario: Timer dispara repetidamente após a inicialização
- **WHEN** o timer do APIC é inicializado
- **THEN** ele SHALL gerar mais de uma interrupção ao longo do tempo de execução do kernel, em intervalos aproximadamente iguais, em vez de disparar uma única vez

### Requirement: Contagem de Ticks
O kernel SHALL manter um contador de ticks, incrementado a cada interrupção do timer, monotonicamente crescente e consultável por outras partes do kernel.

#### Scenario: Contador de ticks aumenta de forma consistente com o tempo decorrido
- **WHEN** o timer do APIC dispara repetidamente após a inicialização
- **THEN** o contador de ticks exposto pelo kernel SHALL aumentar a cada disparo, nunca diminuir, e refletir aproximadamente o tempo decorrido dado o intervalo configurado do timer
