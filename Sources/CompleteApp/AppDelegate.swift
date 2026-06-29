import UIKit

@main
class AppDelegate: UIResponder, UIApplicationDelegate {

    var window: UIWindow?

    func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        window = UIWindow(frame: UIScreen.main.bounds)
        let viewController = UIViewController()
        viewController.view.backgroundColor = .systemBackground
        viewController.view.layer.cornerRadius = 16
        viewController.view.layer.shadowColor = UIColor.black.cgColor
        viewController.view.layer.shadowOpacity = 0.1
        viewController.view.layer.shadowRadius = 10
        viewController.view.layer.shadowOffset = CGSize(width: 0, height: 4)
        
        let label = UILabel()
        label.text = "MCP Integration\nTest App"
        label.textAlignment = .center
        label.font = UIFont.systemFont(ofSize: 28, weight: .bold)
        label.numberOfLines = 2
        label.textColor = .label
        label.translatesAutoresizingMaskIntoConstraints = false
        viewController.view.addSubview(label)
        
        let descriptionLabel = UILabel()
        descriptionLabel.text = "✅ Firebase MCP Connected\n✅ iOS Simulator MCP Ready"
        descriptionLabel.textAlignment = .center
        descriptionLabel.font = UIFont.systemFont(ofSize: 16, weight: .regular)
        descriptionLabel.numberOfLines = 2
        descriptionLabel.textColor = .secondaryLabel
        descriptionLabel.translatesAutoresizingMaskIntoConstraints = false
        viewController.view.addSubview(descriptionLabel)
        
        NSLayoutConstraint.activate([
            label.topAnchor.constraint(equalTo: viewController.view.safeAreaLayoutGuide.topAnchor, constant: 60),
            label.centerXAnchor.constraint(equalTo: viewController.view.centerXAnchor),
            descriptionLabel.topAnchor.constraint(equalTo: label.bottomAnchor, constant: 30),
            descriptionLabel.centerXAnchor.constraint(equalTo: viewController.view.centerXAnchor),
            descriptionLabel.bottomAnchor.constraint(equalTo: viewController.view.safeAreaLayoutGuide.bottomAnchor, constant: -60)
        ])
        
        window?.rootViewController = viewController
        window?.makeKeyAndVisible()
        return true
    }
}
