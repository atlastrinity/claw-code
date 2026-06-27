import SwiftUI
import MapKit

struct ContentView: View {
    // Координати для Києва (як на макеті)
    let centerCoordinate = CLLocationCoordinate2D(latitude: 50.4501, longitude: 30.5234)
    let userCoordinate = CLLocationCoordinate2D(latitude: 50.4450, longitude: 30.5300)
    let shelterCoordinate = CLLocationCoordinate2D(latitude: 50.4520, longitude: 30.5150)
    
    // Стан для анімацій (пульсація)
    @State private var isPulsating = false
    
    var body: some View {
        ZStack(alignment: .top) {
            // 1. ШАР КАРТИ
            Map(initialPosition: .region(MKCoordinateRegion(center: centerCoordinate, span: MKCoordinateSpan(latitudeDelta: 0.04, longitudeDelta: 0.04)))) {
                
                // Радарні кільця (Епіцентр тривоги)
                Annotation("", coordinate: centerCoordinate) {
                    ZStack {
                        Circle()
                            .stroke(Color.red, lineWidth: 1)
                            .frame(width: isPulsating ? 400 : 50)
                            .opacity(isPulsating ? 0 : 0.8)
                        Circle()
                            .stroke(Color.red, lineWidth: 2)
                            .frame(width: isPulsating ? 250 : 20)
                            .opacity(isPulsating ? 0 : 1)
                        Circle()
                            .fill(Color.red.opacity(0.2))
                            .frame(width: 250)
                        
                        Circle()
                            .fill(Color.red)
                            .frame(width: 12, height: 12)
                            .overlay(Circle().stroke(Color.white, lineWidth: 2))
                    }
                }
                
                // Маркер користувача
                Annotation("Ви", coordinate: userCoordinate) {
                    Image(systemName: "location.north.fill")
                        .foregroundColor(.white)
                        .padding(8)
                        .background(Color.green)
                        .clipShape(Circle())
                        .overlay(Circle().stroke(Color.white, lineWidth: 2))
                        .shadow(radius: 5)
                }
                
                // Маркер укриття
                Annotation("Укриття", coordinate: shelterCoordinate) {
                    Image(systemName: "shield.fill")
                        .foregroundColor(.white)
                        .padding(6)
                        .background(Color.blue)
                        .clipShape(Circle())
                        .overlay(Circle().stroke(Color.white, lineWidth: 2))
                }
            }
            .mapStyle(.standard(elevation: .realistic))
            // Затемнюємо карту для акценту на небезпеці (Dark Mode)
            .colorScheme(.dark)
            .ignoresSafeArea()
            
            // Червоний градієнт-віньєтка по краях для тривожності
            RadialGradient(gradient: Gradient(colors: [.clear, .red.opacity(0.3)]), center: .center, startRadius: 100, endRadius: 500)
                .ignoresSafeArea()
                .allowsHitTesting(false)
            
            // 2. ВЕРХНІЙ БАНЕР (Імітація Dynamic Island)
            TopAlertBanner()
                .padding(.top, 10) // Відступ від верхнього краю
            
            // 3. НИЖНЯ ПАНЕЛЬ (Dashboard)
            VStack {
                Spacer()
                BottomDashboard(isPulsating: isPulsating)
            }
            .padding(.bottom, 20)
        }
        .onAppear {
            // Запуск безкінечної анімації
            withAnimation(.easeOut(duration: 2.0).repeatForever(autoreverses: false)) {
                isPulsating = true
            }
        }
    }
}

// MARK: - Верхній банер
struct TopAlertBanner: View {
    var body: some View {
        HStack(spacing: 16) {
            Image(systemName: "bell.badge.fill")
                .foregroundColor(.red)
                .font(.title2)
                .symbolEffect(.bounce, options: .repeating) // iOS 17 анімація іконки
            
            VStack(spacing: 2) {
                Text("ТРИВОГА")
                    .font(.system(size: 12, weight: .bold))
                    .foregroundColor(.red)
                Text("00:15:22")
                    .font(.system(size: 18, weight: .black, design: .monospaced))
                    .foregroundColor(.red)
            }
        }
        .padding(.horizontal, 30)
        .padding(.vertical, 12)
        .background(Color.black.opacity(0.85))
        .clipShape(Capsule())
        .overlay(
            Capsule().stroke(Color.red.opacity(0.5), lineWidth: 1)
        )
        .shadow(color: .red.opacity(0.3), radius: 10, x: 0, y: 5)
    }
}

// MARK: - Нижня скляна панель (Dashboard)
struct BottomDashboard: View {
    var isPulsating: Bool
    
    var body: some View {
        HStack(alignment: .top) {
            // Ліва частина: Статус
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Circle()
                        .fill(Color.red)
                        .frame(width: 12, height: 12)
                        .opacity(isPulsating ? 0.3 : 1.0) // Мигаюча крапка
                        .animation(.easeInOut(duration: 0.5).repeatForever(), value: isPulsating)
                    
                    Text("ПОВІТРЯНА\nТРИВОГА")
                        .font(.system(size: 20, weight: .heavy, design: .default))
                        .foregroundColor(.white)
                        .lineLimit(2)
                }
                
                VStack(alignment: .leading, spacing: 2) {
                    Text("Київ та область")
                        .font(.system(size: 14, weight: .medium))
                        .foregroundColor(.gray)
                    Text("Небезпека: Балістика")
                        .font(.system(size: 14, weight: .regular))
                        .foregroundColor(.gray)
                }
                .padding(.top, 4)
            }
            
            Spacer()
            
            // Права частина: Кнопки
            VStack(alignment: .trailing, spacing: 12) {
                // Маленькі іконки дій
                HStack(spacing: 20) {
                    SmallIconButton(iconName: "arrow.triangle.turn.up.right.diamond.fill")
                    SmallIconButton(iconName: "square.and.arrow.up")
                    SmallIconButton(iconName: "gearshape.fill")
                }
                
                // Головна кнопка "Знайти укриття"
                Button(action: {
                    // Дія прокладання маршруту
                    print("FIND_SHELTER_TAPPED")
                }) {
                    Text("ЗНАЙТИ НАЙБЛИЖЧЕ\nУКРИТТЯ")
                        .font(.system(size: 12, weight: .bold))
                        .multilineTextAlignment(.center)
                        .foregroundColor(.black)
                        .padding(.vertical, 12)
                        .padding(.horizontal, 16)
                        .background(Color(red: 0.6, green: 0.7, blue: 0.9)) // Світло-синій як на макеті
                        .cornerRadius(12)
                }
            }
        }
        .padding(20)
        // Ефект "Матового скла" (Glassmorphism)
        .background(.ultraThinMaterial)
        // Додатковий темний фон для кращої читабельності
        .background(Color.black.opacity(0.4)) 
        .cornerRadius(28)
        .overlay(
            RoundedRectangle(cornerRadius: 28)
                .stroke(Color.white.opacity(0.2), lineWidth: 1)
        )
        .padding(.horizontal, 16)
        .shadow(color: .black.opacity(0.3), radius: 20, x: 0, y: 10)
    }
}

// Допоміжний компонент для дрібних кнопок
struct SmallIconButton: View {
    let iconName: String
    
    var body: some View {
        Button(action: {
            print("SMALL_ICON_TAPPED_\(iconName)")
        }) {
            Image(systemName: iconName)
                .font(.system(size: 16))
                .foregroundColor(.white.opacity(0.8))
                .frame(width: 30, height: 30)
                .background(Color.white.opacity(0.1))
                .clipShape(Circle())
        }
    }
}

#Preview {
    ContentView()
}
