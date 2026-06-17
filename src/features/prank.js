import { listen, invoke } from '../lib/core.js';
import catImgUrl from '../assets/prank_cat.png';

// ==========================================
// НАСТРОЙКА СОБСТВЕННОГО ЗВУКА:
// 1. Положите ваш mp3-файл в папку: src/assets/flash.mp3
// 2. Раскомментируйте строчку ниже (уберите два слэша //):
import flashSoundUrl from '../assets/flash.mp3';
// ==========================================

function playScreenshotSound() {
  // ==========================================
  // Если вы раскомментировали импорт звука выше,
  // раскомментируйте этот блок кода (уберите /* и */)
  // для проигрывания вашего MP3 файла:
  try {
    const audio = new Audio(flashSoundUrl);
    audio.volume = 1.0;
    audio.play();
    return; // Выходим, чтобы встроенный синтезатор звука не играл
  } catch (e) {
    // Ошибки логируем молча
  }
  // ==========================================

  try {
    const ctx = new (window.AudioContext || window.webkitAudioContext)();
    
    // Встроенный синтезатор: звук уведомления Windows (A5 -> C#6 -> E6 -> A6)
    const now = ctx.currentTime;
    
    const playTone = (freq, start, duration) => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      
      osc.type = 'sine';
      osc.frequency.setValueAtTime(freq, start);
      
      gain.gain.setValueAtTime(0.0, start);
      gain.gain.linearRampToValueAtTime(0.15, start + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + duration);
      
      osc.connect(gain);
      gain.connect(ctx.destination);
      
      osc.start(start);
      osc.stop(start + duration);
    };
    
    playTone(880, now, 0.4);        // A5
    playTone(1109, now + 0.08, 0.4); // C#6
    playTone(1318, now + 0.16, 0.5); // E6
    playTone(1760, now + 0.24, 0.6); // A6
  } catch (e) {
    // Ошибки глушим
  }
}

export function initPrank() {
  listen('game-started', async () => {
    try {
      // Воспроизводим звук
      playScreenshotSound();
      
      // Создаем оверлей
      const container = document.createElement('div');
      container.id = 'prank-overlay';
      container.style.position = 'fixed';
      container.style.top = '0';
      container.style.left = '0';
      container.style.width = '100vw';
      container.style.height = '100vh';
      container.style.zIndex = '999999';
      container.style.backgroundColor = '#000000';
      container.style.display = 'flex';
      container.style.alignItems = 'center';
      container.style.justifyContent = 'center';
      container.style.overflow = 'hidden';
      
      // Картинка на весь экран
      const img = document.createElement('img');
      img.src = catImgUrl;
      img.style.width = '100vw';
      img.style.height = '100vh';
      img.style.objectFit = 'contain';
      img.style.opacity = '0';
      img.style.transform = 'scale(0.8)';
      img.style.transition = 'opacity 0.5s ease-out, transform 0.5s ease-out';
      container.appendChild(img);
      
      // Вспышка (белый экран)
      const flash = document.createElement('div');
      flash.style.position = 'absolute';
      flash.style.top = '0';
      flash.style.left = '0';
      flash.style.width = '100%';
      flash.style.height = '100%';
      flash.style.backgroundColor = '#ffffff';
      flash.style.zIndex = '1000000';
      flash.style.transition = 'opacity 1s cubic-bezier(0.1, 0.8, 0.3, 1)';
      flash.style.opacity = '1';
      container.appendChild(flash);
      
      document.body.appendChild(container);
      
      // Запуск затухания вспышки
      setTimeout(() => {
        flash.style.opacity = '0';
      }, 50);
      
      // Показ картинки кота после вспышки
      setTimeout(() => {
        img.style.opacity = '1';
        img.style.transform = 'scale(1.0)';
      }, 200);
      
      // Удаление элемента вспышки
      setTimeout(() => {
        if (flash.parentNode) {
          flash.parentNode.removeChild(flash);
        }
      }, 1100);
      
      let timeoutId;
      let isCleanedUp = false;
      
      const cleanupPrank = async () => {
        if (isCleanedUp) return;
        isCleanedUp = true;
        
        if (timeoutId) clearTimeout(timeoutId);
        document.removeEventListener('keydown', handleKeyDown);
        
        container.style.transition = 'opacity 0.5s ease-in-out';
        container.style.opacity = '0';
        
        // Даем команду бэкенду свернуть окно
        await invoke('close_prank');
        
        setTimeout(() => {
          if (container.parentNode) {
            container.parentNode.removeChild(container);
          }
        }, 500);
      };
      
      const handleKeyDown = (e) => {
        if (e.key === 'Escape') {
          cleanupPrank();
        }
      };
      
      document.addEventListener('keydown', handleKeyDown);
      
      // Закрываем через 10 секунд
      timeoutId = setTimeout(cleanupPrank, 10000);
      
    } catch (e) {
      // Ошибки глушим
    }
  });
}
