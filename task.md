# Task Roadmap: Claw-Code iOS Remote Client

## 1. Проєктна інфраструктура та моделі даних [pending]
- [x] 1.1. Аналіз структури проєкту `ClawController`
- [x] 1.2. Створення базових структур даних (Models) для WebSocket-повідомлень (TaskNode, etc)
- [ ] 1.3. Реалізація `Decodable` для отриманих JSON-даних [pending]

## 2. Мережевий рівень (Networking) [pending]
- [ ] 2.1. Створення `WebSocketManager` [pending]
- [ ] 2.2. Реалізація механізму реконнекту [pending]
- [ ] 2.3. Створення потоку даних (ObservableObject) для оновлення UI [pending]

## 3. UI/UX: Чат та Моніторинг [pending]
- [ ] 3.1. Створення базового SwiftUI-інтерфейсу чату (ChatView) [pending]
- [ ] 3.2. Розробка компоненту для відображення ієрархії завдань (TaskDashboardView) [pending]
- [ ] 3.3. Реалізація відображення статусів (pending, in_progress, completed, failed) [pending]

## 4. Фільтрація та логування [pending]
- [ ] 4.1. Створення інтерфейсу фільтрації логів [pending]
- [ ] 4.2. Інтеграція фільтрів у стрім даних [pending]

## 5. Тестування та Верифікація [pending]
- [ ] 5.1. Мокове тестування WebSocket-з'єднання [pending]
- [ ] 5.2. Верифікація відображення Task Graph в UI [pending]
